//! Apple's on-device speech analyzer, through the Swift bridge `build.rs`
//! links on macOS (`swift/WritOnDevice`, `Dictation.swift`).
//!
//! What the bridge's failure codes mean is [`writ_core::dictation::on_device`];
//! running a session on it is [`super::session`]. This module is the call and
//! the callback. A build without the bridge, and every platform other than
//! macOS, answers as the bridge does before macOS 26: on-device dictation is
//! unsupported, and the microphone is reported as allowed so capture errors
//! speak for it.

use tokio::sync::mpsc;

pub use native::Handle as Recognition;

/// One message from an on-device recognition. Exactly one of `Failed`,
/// `Stopped` or `Finished` ends it, and the channel closes after that.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeEvent {
    /// The speech model is downloading; the fraction done, when known.
    Preparing(Option<f32>),
    /// Samples may be fed.
    Ready,
    /// The words heard so far, not final yet.
    Partial(String),
    /// A finished stretch of text.
    Final(String),
    /// The recognition failed with a bridge code.
    Failed(i32),
    /// The recognition was stopped.
    Stopped,
    /// Every word of the input has arrived.
    Finished,
}

impl BridgeEvent {
    /// Whether this event ends the recognition.
    pub fn is_ending(&self) -> bool {
        matches!(self, Self::Failed(_) | Self::Stopped | Self::Finished)
    }
}

/// Whether the system lets the app use the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicStatus {
    /// Allowed.
    Authorized,
    /// Not asked yet.
    NotDetermined,
    /// Turned off by the person.
    Denied,
    /// Turned off by policy.
    Restricted,
}

impl MicStatus {
    /// The status for a bridge code; an unknown code reads as denied.
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Authorized,
            1 => Self::NotDetermined,
            3 => Self::Restricted,
            _ => Self::Denied,
        }
    }
}

const KIND_PARTIAL: i32 = 0;
const KIND_FINAL: i32 = 1;
const KIND_FAILED: i32 = 2;
const KIND_STOPPED: i32 = 3;
const KIND_FINISHED: i32 = 4;
const KIND_READY: i32 = 5;
const KIND_PREPARING: i32 = 6;

/// The event for one callback from `writ_dictation_start`; `None` for a kind
/// the bridge does not send.
fn decode(kind: i32, code: i32, text: Option<String>) -> Option<BridgeEvent> {
    let event = match kind {
        KIND_PARTIAL => BridgeEvent::Partial(text.unwrap_or_default()),
        KIND_FINAL => BridgeEvent::Final(text.unwrap_or_default()),
        KIND_FAILED => BridgeEvent::Failed(code),
        KIND_STOPPED => BridgeEvent::Stopped,
        KIND_FINISHED => BridgeEvent::Finished,
        KIND_READY => BridgeEvent::Ready,
        KIND_PREPARING => BridgeEvent::Preparing((code >= 0).then(|| code.min(100) as f32 / 100.0)),
        _ => return None,
    };
    Some(event)
}

/// What the system says about the microphone right now.
pub fn mic_status() -> MicStatus {
    MicStatus::from_code(native::mic_status_code())
}

/// Asks the system for the microphone and answers whether it was granted.
pub async fn request_mic() -> bool {
    native::request_mic().await
}

/// The locales on-device dictation supports, as BCP 47 identifiers, or the
/// bridge's failure code.
pub async fn supported_locales() -> Result<Vec<String>, i32> {
    native::supported_locales().await
}

/// The person's first preferred language as a locale identifier; empty where
/// there is no bridge.
pub fn system_locale() -> String {
    native::system_locale()
}

/// Starts recognizing speech in `locale`. Feed 16 kHz mono samples through
/// the handle once [`BridgeEvent::Ready`] arrives; dropping the handle stops
/// the recognition.
pub fn start(locale: &str) -> (Recognition, mpsc::UnboundedReceiver<BridgeEvent>) {
    native::start(locale)
}

#[cfg(writ_on_device_bridge)]
mod native {
    use std::ffi::{c_char, c_void, CStr, CString};

    use tokio::sync::{mpsc, oneshot};

    use super::{decode, BridgeEvent, KIND_FAILED, KIND_FINISHED, KIND_PARTIAL, KIND_STOPPED};

    type Callback = extern "C" fn(*mut c_void, i32, i32, *const c_char);
    type Sender = mpsc::UnboundedSender<BridgeEvent>;

    extern "C" {
        fn writ_dictation_mic_status() -> i32;
        fn writ_dictation_mic_request(context: *mut c_void, callback: Callback);
        fn writ_dictation_supported_locales(context: *mut c_void, callback: Callback);
        fn writ_dictation_system_locale() -> *mut c_char;
        fn writ_dictation_free_string(pointer: *mut c_char);
        fn writ_dictation_start(
            locale: *const c_char,
            context: *mut c_void,
            callback: Callback,
        ) -> *mut c_void;
        fn writ_dictation_feed(handle: *mut c_void, samples: *const i16, count: isize);
        fn writ_dictation_finish(handle: *mut c_void);
        fn writ_dictation_stop(handle: *mut c_void);
        fn writ_dictation_release(handle: *mut c_void);
    }

    /// The text of a callback, or `None` for a null pointer.
    ///
    /// # Safety
    /// `text` is null or a NUL-terminated string that lives for the call.
    unsafe fn callback_text(text: *const c_char) -> Option<String> {
        if text.is_null() {
            return None;
        }
        // SAFETY: the caller guarantees a live NUL-terminated string.
        Some(
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    pub(super) fn mic_status_code() -> i32 {
        // SAFETY: takes no arguments and reads the process's microphone
        // authorization.
        unsafe { writ_dictation_mic_status() }
    }

    /// Receives the one answer to a microphone request. The context is a
    /// boxed sender the answer takes back, so it is freed exactly once.
    extern "C" fn on_mic_answer(context: *mut c_void, _kind: i32, code: i32, _text: *const c_char) {
        if context.is_null() {
            return;
        }
        // SAFETY: the bridge answers exactly once, with the box
        // `request_mic` made.
        let sender = unsafe { Box::from_raw(context as *mut oneshot::Sender<bool>) };
        let _ = sender.send(code == 1);
    }

    pub(super) async fn request_mic() -> bool {
        let (sender, receiver) = oneshot::channel::<bool>();
        let context = Box::into_raw(Box::new(sender)) as *mut c_void;
        // SAFETY: the context is a live box the callback frees on its one
        // answer.
        unsafe { writ_dictation_mic_request(context, on_mic_answer) };
        receiver.await.unwrap_or(false)
    }

    /// The locales listed so far and where the finished list goes.
    struct LocaleList {
        found: Vec<String>,
        done: oneshot::Sender<Result<Vec<String>, i32>>,
    }

    /// Receives one locale, or the end of the list. Each locale borrows the
    /// context; the end takes it back, so it is freed exactly once.
    extern "C" fn on_locale(context: *mut c_void, kind: i32, code: i32, text: *const c_char) {
        if context.is_null() {
            return;
        }
        if kind == KIND_PARTIAL {
            // SAFETY: the bridge sends the locales one after another from one
            // task, before the end that frees the context.
            let list = unsafe { &mut *(context as *mut LocaleList) };
            // SAFETY: the bridge passes a NUL-terminated string that lives
            // for the length of this call.
            if let Some(locale) = unsafe { callback_text(text) } {
                list.found.push(locale);
            }
            return;
        }
        // SAFETY: the end comes exactly once, after every locale, with the box
        // `supported_locales` made.
        let list = unsafe { Box::from_raw(context as *mut LocaleList) };
        let LocaleList { found, done } = *list;
        let answer = if kind == KIND_FAILED {
            Err(code)
        } else {
            Ok(found)
        };
        let _ = done.send(answer);
    }

    pub(super) async fn supported_locales() -> Result<Vec<String>, i32> {
        let (done, receiver) = oneshot::channel();
        let list = LocaleList {
            found: Vec::new(),
            done,
        };
        let context = Box::into_raw(Box::new(list)) as *mut c_void;
        // SAFETY: the context is a live box the callback frees on the end of
        // the list.
        unsafe { writ_dictation_supported_locales(context, on_locale) };
        receiver
            .await
            .unwrap_or(Err(writ_core::dictation::on_device::BRIDGE_UNSUPPORTED))
    }

    pub(super) fn system_locale() -> String {
        // SAFETY: takes no arguments; the result is null or a string the
        // bridge allocated for this caller.
        let pointer = unsafe { writ_dictation_system_locale() };
        if pointer.is_null() {
            return String::new();
        }
        // SAFETY: the pointer is a live NUL-terminated string until it is
        // freed below.
        let locale = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: freed once, by the function the bridge names for it.
        unsafe { writ_dictation_free_string(pointer) };
        locale
    }

    /// The bridge's handle on one recognition. Dropping it stops the
    /// recognition, which ends the event channel, and releases it.
    pub struct Handle(*mut c_void);

    // SAFETY: the Swift session behind the handle guards its state with a
    // lock, and every call on it may come from any thread.
    unsafe impl Send for Handle {}

    impl Handle {
        /// Feeds 16 kHz mono samples. Samples fed before the ready event are
        /// dropped by the bridge.
        pub fn feed(&self, samples: &[i16]) {
            let Ok(count) = isize::try_from(samples.len()) else {
                return;
            };
            if count == 0 {
                return;
            }
            // SAFETY: the handle is live until `drop`; the bridge copies the
            // samples before it returns.
            unsafe { writ_dictation_feed(self.0, samples.as_ptr(), count) }
        }

        /// Ends the input: the text still pending arrives, then the finished
        /// event.
        pub fn finish(&self) {
            // SAFETY: the handle is live until `drop` releases it.
            unsafe { writ_dictation_finish(self.0) }
        }

        /// Stops at once; the recognition ends with the stopped event unless
        /// it already ended.
        pub fn stop(&self) {
            // SAFETY: the handle is live until `drop` releases it.
            unsafe { writ_dictation_stop(self.0) }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // The stop guarantees the ending event that frees the context.
            self.stop();
            // SAFETY: released once, here; the bridge keeps the session alive
            // for as long as it still calls back.
            unsafe { writ_dictation_release(self.0) }
        }
    }

    /// Receives one event. The context is a boxed sender: every event before
    /// the end borrows it, and the one event that ends the recognition takes
    /// it back and drops it, so it is freed exactly once.
    extern "C" fn on_event(context: *mut c_void, kind: i32, code: i32, text: *const c_char) {
        if context.is_null() {
            return;
        }
        // SAFETY: the bridge passes a null or NUL-terminated string that
        // lives for the length of this call.
        let event = decode(kind, code, unsafe { callback_text(text) });
        if matches!(kind, KIND_FAILED | KIND_STOPPED | KIND_FINISHED) {
            // SAFETY: the ending event comes exactly once, after every other
            // event, and the box was made by `start` for this recognition.
            let sender = unsafe { Box::from_raw(context as *mut Sender) };
            if let Some(event) = event {
                let _ = sender.send(event);
            }
            return;
        }
        // SAFETY: the context outlives every event before the end; only the
        // ending event frees it, and the bridge sends nothing after that.
        let sender = unsafe { &*(context as *const Sender) };
        if let Some(event) = event {
            let _ = sender.send(event);
        }
    }

    /// A C string of `text`. A NUL inside the text ends a C string early, so
    /// it is dropped.
    fn c_text(text: &str) -> CString {
        CString::new(text.replace('\0', "")).unwrap_or_default()
    }

    pub(super) fn start(locale: &str) -> (Handle, mpsc::UnboundedReceiver<BridgeEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let context = Box::into_raw(Box::new(sender)) as *mut c_void;
        let locale = c_text(locale);
        // SAFETY: the locale outlives the call, which copies it; the context
        // is a live box the callback frees on the ending event.
        let handle = unsafe { writ_dictation_start(locale.as_ptr(), context, on_event) };
        (Handle(handle), receiver)
    }
}

#[cfg(not(writ_on_device_bridge))]
mod native {
    use tokio::sync::mpsc;
    use writ_core::dictation::on_device::BRIDGE_UNSUPPORTED;

    use super::{decode, BridgeEvent, KIND_FAILED};

    pub(super) fn mic_status_code() -> i32 {
        0
    }

    pub(super) async fn request_mic() -> bool {
        true
    }

    pub(super) async fn supported_locales() -> Result<Vec<String>, i32> {
        Err(BRIDGE_UNSUPPORTED)
    }

    pub(super) fn system_locale() -> String {
        String::new()
    }

    /// A recognition that ended before it began.
    pub struct Handle;

    impl Handle {
        /// Nothing to feed.
        pub fn feed(&self, _samples: &[i16]) {}

        /// Nothing to finish.
        pub fn finish(&self) {}

        /// Nothing to stop.
        pub fn stop(&self) {}
    }

    pub(super) fn start(_locale: &str) -> (Handle, mpsc::UnboundedReceiver<BridgeEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        if let Some(event) = decode(KIND_FAILED, BRIDGE_UNSUPPORTED, None) {
            let _ = sender.send(event);
        }
        (Handle, receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_the_bridge_sends_has_its_event() {
        let text = || Some("words".to_string());
        assert_eq!(
            decode(KIND_PARTIAL, 0, text()),
            Some(BridgeEvent::Partial("words".to_string()))
        );
        assert_eq!(
            decode(KIND_FINAL, 0, text()),
            Some(BridgeEvent::Final("words".to_string()))
        );
        assert_eq!(
            decode(KIND_FINAL, 0, None),
            Some(BridgeEvent::Final(String::new()))
        );
        assert_eq!(decode(KIND_FAILED, 22, None), Some(BridgeEvent::Failed(22)));
        assert_eq!(decode(KIND_STOPPED, 0, None), Some(BridgeEvent::Stopped));
        assert_eq!(decode(KIND_FINISHED, 0, None), Some(BridgeEvent::Finished));
        assert_eq!(decode(KIND_READY, 0, None), Some(BridgeEvent::Ready));
        assert_eq!(
            decode(KIND_PREPARING, 40, None),
            Some(BridgeEvent::Preparing(Some(0.4)))
        );
        assert_eq!(
            decode(KIND_PREPARING, -1, None),
            Some(BridgeEvent::Preparing(None))
        );
        assert_eq!(
            decode(KIND_PREPARING, 140, None),
            Some(BridgeEvent::Preparing(Some(1.0)))
        );
        assert_eq!(decode(9, 0, None), None);
    }

    #[test]
    fn only_failed_stopped_and_finished_end_a_recognition() {
        assert!(BridgeEvent::Failed(1).is_ending());
        assert!(BridgeEvent::Stopped.is_ending());
        assert!(BridgeEvent::Finished.is_ending());
        assert!(!BridgeEvent::Ready.is_ending());
        assert!(!BridgeEvent::Preparing(None).is_ending());
        assert!(!BridgeEvent::Partial(String::new()).is_ending());
        assert!(!BridgeEvent::Final(String::new()).is_ending());
    }

    #[test]
    fn microphone_codes_read_as_their_status() {
        assert_eq!(MicStatus::from_code(0), MicStatus::Authorized);
        assert_eq!(MicStatus::from_code(1), MicStatus::NotDetermined);
        assert_eq!(MicStatus::from_code(2), MicStatus::Denied);
        assert_eq!(MicStatus::from_code(3), MicStatus::Restricted);
        assert_eq!(MicStatus::from_code(7), MicStatus::Denied);
    }

    /// Every event up to and including the end, and that the channel closes
    /// after it.
    fn drain(mut receiver: mpsc::UnboundedReceiver<BridgeEvent>) -> Vec<BridgeEvent> {
        tauri::async_runtime::block_on(async move {
            let mut events = Vec::new();
            while let Some(event) = receiver.recv().await {
                events.push(event);
            }
            events
        })
    }

    #[cfg(not(writ_on_device_bridge))]
    #[test]
    fn without_the_bridge_on_device_dictation_is_unsupported() {
        use writ_core::dictation::on_device::BRIDGE_UNSUPPORTED;

        let locales = tauri::async_runtime::block_on(supported_locales());
        assert_eq!(locales, Err(BRIDGE_UNSUPPORTED));
        assert_eq!(system_locale(), "");
        assert_eq!(mic_status(), MicStatus::Authorized);
        let (_handle, receiver) = start("en-US");
        assert_eq!(
            drain(receiver),
            vec![BridgeEvent::Failed(BRIDGE_UNSUPPORTED)]
        );
    }

    #[cfg(writ_on_device_bridge)]
    #[test]
    fn the_bridge_reads_the_running_system() {
        use writ_core::dictation::on_device::BRIDGE_UNSUPPORTED;

        assert!(!system_locale().trim().is_empty());
        match tauri::async_runtime::block_on(supported_locales()) {
            Ok(locales) => assert!(locales.iter().all(|id| !id.is_empty()), "{locales:?}"),
            Err(code) => assert_eq!(code, BRIDGE_UNSUPPORTED),
        }
    }

    #[cfg(writ_on_device_bridge)]
    #[test]
    fn a_locale_the_bridge_lacks_ends_once_with_its_reason() {
        use writ_core::dictation::on_device::{BRIDGE_LANGUAGE_UNSUPPORTED, BRIDGE_UNSUPPORTED};

        let (handle, receiver) = start("zz-ZZ");
        let events = tauri::async_runtime::block_on(async {
            let mut receiver = receiver;
            let mut events = Vec::new();
            while let Some(event) = receiver.recv().await {
                events.push(event);
            }
            events
        });
        drop(handle);
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(
            matches!(
                events[0],
                BridgeEvent::Failed(BRIDGE_LANGUAGE_UNSUPPORTED | BRIDGE_UNSUPPORTED)
            ),
            "{events:?}"
        );
    }

    #[cfg(writ_on_device_bridge)]
    #[test]
    fn a_stopped_recognition_ends_once_and_closes() {
        let (handle, receiver) = start("zz-ZZ");
        drop(handle);
        let events = drain(receiver);
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(events[0].is_ending(), "{events:?}");
    }

    /// The 16-bit mono samples of a WAV file's `data` chunk.
    fn wav_samples(bytes: &[u8]) -> Vec<i16> {
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let body = at + 8;
            if id == b"data" {
                let end = (body + size).min(bytes.len());
                let (pairs, _) = bytes[body..end].as_chunks::<2>();
                return pairs.iter().map(|pair| i16::from_le_bytes(*pair)).collect();
            }
            at = body + size + (size % 2);
        }
        panic!("the WAV file has no data chunk");
    }

    /// A live recognition of synthesized speech on macOS 26 with the English
    /// model installable: `cargo test -p writ-tauri --lib dictation::bridge -- --ignored`.
    #[test]
    #[ignore = "needs macOS 26 and the English speech model"]
    fn synthesized_speech_comes_back_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("speech.wav");
        let said = std::process::Command::new("say")
            .arg("-o")
            .arg(&path)
            .arg("--data-format=LEI16@16000")
            .arg("Writing with my voice is faster than typing.")
            .status()
            .unwrap();
        assert!(said.success());
        let mut samples = wav_samples(&std::fs::read(&path).unwrap());
        samples.extend(std::iter::repeat_n(0, 16_000));

        let text = tauri::async_runtime::block_on(async move {
            let locales = supported_locales().await.expect("on-device dictation");
            let locale = writ_core::dictation::on_device_locale("en", &system_locale(), &locales)
                .expect("an English locale");
            let (handle, mut receiver) = start(&locale);
            let mut text = String::new();
            let mut fed = false;
            let deadline = std::time::Duration::from_secs(300);
            loop {
                let event = tokio::time::timeout(deadline, receiver.recv())
                    .await
                    .expect("the recognition answered in time");
                match event {
                    Some(BridgeEvent::Ready) if !fed => {
                        for chunk in samples.chunks(320) {
                            handle.feed(chunk);
                        }
                        handle.finish();
                        fed = true;
                    }
                    Some(BridgeEvent::Final(piece)) => text.push_str(&piece),
                    Some(BridgeEvent::Finished) | None => break,
                    Some(BridgeEvent::Failed(code)) => panic!("failed with {code}"),
                    Some(_) => {}
                }
            }
            text
        });
        let lower = text.to_lowercase();
        assert!(
            lower.contains("voice") || lower.contains("typing"),
            "{text}"
        );
    }
}
