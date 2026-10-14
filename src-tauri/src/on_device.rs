//! Apple's on-device model, through the Swift bridge `build.rs` links on
//! macOS (`swift/WritOnDevice`).
//!
//! What the bridge's answers mean is [`writ_core::ai::on_device`]; turning its
//! events into the rewrite stream is [`writ_ai::connection::OnDeviceReply`].
//! This module is the call and the callback. A build without the bridge, and
//! every platform other than macOS, reports the model as unsupported.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use writ_ai::connection::{OnDeviceEvent, OnDeviceReply, OnDeviceRequest, StreamEvent};
use writ_core::ai::on_device::{offers_on_device, OnDeviceAvailability, OnDeviceFailure};

/// What the system reports about the on-device model right now.
pub fn availability() -> OnDeviceAvailability {
    OnDeviceAvailability::from_code(bridge::availability_code())
}

/// Whether the provider table shows the on-device row on this machine.
pub fn is_offered() -> bool {
    offers_on_device(bridge::os_major(), availability())
}

/// Streams one rewrite through the bridge, invoking `on_event` for each piece,
/// the end, or a failure.
///
/// `cancel` stops the request wherever it is, and nothing further is emitted.
pub async fn run_rewrite(
    request: &OnDeviceRequest,
    cancel: &CancellationToken,
    mut on_event: impl FnMut(StreamEvent),
) {
    let mut stream = bridge::start(request);
    let mut reply = OnDeviceReply::default();
    loop {
        let next = cancel.run_until_cancelled(stream.next()).await;
        // A stop that lands with an event is a stop: nothing follows it.
        let Some(next) = next.filter(|_| !cancel.is_cancelled()) else {
            stream.stop();
            return;
        };
        let Some(event) = next else {
            // The bridge ended the stream without a reply or a reason, which
            // only a stop it did not hear about from here can do.
            on_event(StreamEvent::Error(OnDeviceFailure::Failed.to_string()));
            return;
        };
        let last = !matches!(event, OnDeviceEvent::Snapshot(_));
        if let Some(stream_event) = reply.event(event) {
            let failed = matches!(stream_event, StreamEvent::Error(_));
            on_event(stream_event);
            if failed {
                stream.stop();
                return;
            }
        }
        if last {
            return;
        }
    }
}

/// The events of one request, as the bridge's callback delivers them.
struct Events {
    receiver: mpsc::UnboundedReceiver<OnDeviceEvent>,
    handle: bridge::Handle,
}

impl Events {
    async fn next(&mut self) -> Option<OnDeviceEvent> {
        self.receiver.recv().await
    }

    fn stop(&self) {
        self.handle.stop();
    }
}

#[cfg(writ_on_device_bridge)]
mod bridge {
    use std::ffi::{c_char, c_void, CStr, CString};

    use tokio::sync::mpsc;
    use writ_ai::connection::{OnDeviceEvent, OnDeviceRequest};

    use super::Events;

    type Callback = extern "C" fn(*mut c_void, i32, i32, *const c_char);
    type Sender = mpsc::UnboundedSender<OnDeviceEvent>;

    extern "C" {
        fn writ_on_device_os_major() -> i32;
        fn writ_on_device_availability() -> i32;
        fn writ_on_device_stream(
            instructions: *const c_char,
            prompt: *const c_char,
            temperature: f64,
            context: *mut c_void,
            callback: Callback,
        ) -> *mut c_void;
        fn writ_on_device_stop(handle: *mut c_void);
        fn writ_on_device_release(handle: *mut c_void);
    }

    const EVENT_SNAPSHOT: i32 = 0;
    const EVENT_DONE: i32 = 1;
    const EVENT_FAILED: i32 = 2;

    pub(super) fn os_major() -> u32 {
        // SAFETY: takes no arguments and reads the process's system version.
        u32::try_from(unsafe { writ_on_device_os_major() }).unwrap_or(0)
    }

    pub(super) fn availability_code() -> i32 {
        // SAFETY: takes no arguments; the bridge answers a code.
        unsafe { writ_on_device_availability() }
    }

    /// The bridge's handle on one request, released when dropped.
    pub(super) struct Handle(*mut c_void);

    // SAFETY: the Swift object behind the handle guards its state with a lock,
    // and stop and release may be called from any thread.
    unsafe impl Send for Handle {}

    impl Handle {
        pub(super) fn stop(&self) {
            // SAFETY: the handle is live until `drop` releases it.
            unsafe { writ_on_device_stop(self.0) }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: released once, here; the bridge keeps the request alive
            // for as long as it still calls back.
            unsafe { writ_on_device_release(self.0) }
        }
    }

    /// Receives one event. The context is a boxed sender: a snapshot borrows
    /// it, and the one event that ends the stream takes it back and drops it,
    /// so it is freed exactly once and never touched again.
    extern "C" fn on_event(context: *mut c_void, kind: i32, code: i32, text: *const c_char) {
        if context.is_null() {
            return;
        }
        if kind == EVENT_SNAPSHOT {
            // SAFETY: the context outlives every snapshot; only the ending
            // event frees it, and the bridge sends nothing after that.
            let sender = unsafe { &*(context as *const Sender) };
            let snapshot = if text.is_null() {
                String::new()
            } else {
                // SAFETY: the bridge passes a NUL-terminated string that lives
                // for the length of this call.
                unsafe { CStr::from_ptr(text) }
                    .to_string_lossy()
                    .into_owned()
            };
            let _ = sender.send(OnDeviceEvent::Snapshot(snapshot));
            return;
        }
        // SAFETY: the ending event comes exactly once, after every snapshot,
        // and the box was made by `start` for this request.
        let sender = unsafe { Box::from_raw(context as *mut Sender) };
        let _ = match kind {
            EVENT_DONE => sender.send(OnDeviceEvent::Done),
            EVENT_FAILED => sender.send(OnDeviceEvent::Failed(code)),
            // A stop: the receiver is gone or going, and closing the channel
            // is all there is to say.
            _ => Ok(()),
        };
    }

    /// A C string of `text`. A NUL inside the text ends a C string early, so
    /// it is dropped rather than cutting the request short.
    fn c_text(text: &str) -> CString {
        CString::new(text.replace('\0', "")).unwrap_or_default()
    }

    pub(super) fn start(request: &OnDeviceRequest) -> Events {
        let (sender, receiver) = mpsc::unbounded_channel();
        let context = Box::into_raw(Box::new(sender)) as *mut c_void;
        let instructions = c_text(&request.instructions);
        let prompt = c_text(&request.prompt);
        // SAFETY: both strings outlive the call, which copies them; the
        // context is a live box the callback frees on the ending event.
        let handle = unsafe {
            writ_on_device_stream(
                instructions.as_ptr(),
                prompt.as_ptr(),
                request.temperature,
                context,
                on_event,
            )
        };
        Events {
            receiver,
            handle: Handle(handle),
        }
    }
}

#[cfg(not(writ_on_device_bridge))]
mod bridge {
    use tokio::sync::mpsc;
    use writ_ai::connection::{OnDeviceEvent, OnDeviceRequest};

    use super::Events;

    /// What the bridge answers for "not usable here".
    const UNSUPPORTED: i32 = 1;

    pub(super) fn os_major() -> u32 {
        0
    }

    pub(super) fn availability_code() -> i32 {
        UNSUPPORTED
    }

    pub(super) struct Handle;

    impl Handle {
        pub(super) fn stop(&self) {}
    }

    pub(super) fn start(_request: &OnDeviceRequest) -> Events {
        let (sender, receiver) = mpsc::unbounded_channel();
        let _ = sender.send(OnDeviceEvent::Failed(UNSUPPORTED));
        Events {
            receiver,
            handle: Handle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str) -> OnDeviceRequest {
        OnDeviceRequest {
            instructions: "Fix the spelling. Return only the text.".to_string(),
            prompt: text.to_string(),
            temperature: 0.3,
        }
    }

    #[test]
    fn the_row_is_offered_exactly_when_the_system_has_the_model() {
        let availability = availability();
        assert_eq!(
            is_offered(),
            offers_on_device(bridge::os_major(), availability)
        );
        if cfg!(not(target_os = "macos")) {
            assert_eq!(availability, OnDeviceAvailability::Unsupported);
            assert!(!is_offered());
        }
    }

    #[cfg(writ_on_device_bridge)]
    #[test]
    fn the_bridge_reads_the_running_system() {
        assert!(bridge::os_major() >= 12, "{}", bridge::os_major());
        let code = bridge::availability_code();
        assert!((0..=5).contains(&code), "{code}");
    }

    #[test]
    fn a_rewrite_the_model_cannot_take_ends_with_its_reason() {
        // Runs where the model cannot answer: another platform, a build
        // without the bridge, or a Mac with Apple Intelligence off. A Mac
        // where it can answer is the manual run.
        let availability = availability();
        let Some(failure) = availability.failure() else {
            return;
        };
        let mut events = Vec::new();
        tauri::async_runtime::block_on(run_rewrite(
            &request("teh text"),
            &CancellationToken::new(),
            |event| events.push(event),
        ));
        assert_eq!(events, vec![StreamEvent::Error(failure.to_string())]);
    }

    #[test]
    fn a_stopped_rewrite_emits_nothing() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut events = Vec::new();
        tauri::async_runtime::block_on(run_rewrite(&request("teh text"), &cancel, |event| {
            events.push(event)
        }));
        assert!(events.is_empty(), "{events:?}");
    }

    /// A live rewrite on a Mac with Apple Intelligence on:
    /// `cargo test -p writ-tauri --lib on_device -- --ignored`.
    #[test]
    #[ignore = "needs Apple Intelligence turned on"]
    fn a_rewrite_streams_on_a_mac_with_the_model() {
        assert_eq!(availability(), OnDeviceAvailability::Available);
        let mut events = Vec::new();
        tauri::async_runtime::block_on(run_rewrite(
            &request("Ths sentense has two mistaks."),
            &CancellationToken::new(),
            |event| events.push(event),
        ));
        assert_eq!(events.last(), Some(&StreamEvent::Done), "{events:?}");
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::Chunk(piece) => Some(piece.as_str()),
                _ => None,
            })
            .collect();
        assert!(!text.trim().is_empty());
    }
}
