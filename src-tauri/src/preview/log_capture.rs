//! Test-only capture of `tracing` events, so a log line a rule depends on
//! can be asserted rather than assumed.
//!
//! One subscriber is installed as the process-wide default the first time a
//! capture runs, and it never comes down; each capture only swaps the sink on
//! its own thread. A thread-local subscriber cannot do this job: `tracing`
//! caches per-callsite interest globally, and it recomputes that cache from
//! whatever the *registering* thread's default is, so any parallel test that
//! first touches a callsite while holding no subscriber turns it off for a
//! capture already running elsewhere — the `logs=[]` failure this replaces.
//! With a default always in place, interest for every callsite is stable and
//! parallel captures share nothing but the routing.
//!
//! The sink is per thread: a capture wrapping code that emits from a thread it
//! spawned, or from a runtime worker, sees none of those events.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

thread_local! {
    /// Where this thread's events go while a capture is open on it.
    static SINK: RefCell<Option<CapturedLogs>> = const { RefCell::new(None) };
}

/// Events recorded while a capture was installed.
#[derive(Clone, Default)]
pub struct CapturedLogs(Arc<Mutex<Vec<String>>>);

impl CapturedLogs {
    /// One line per event: level, then every field including the message.
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().expect("captured logs mutex").clone()
    }
}

/// Flattens an event's fields onto one line.
struct LineVisitor(String);

impl Visit for LineVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

/// The installed layer: it holds no buffer of its own, it hands each event to
/// the sink of the thread that emitted it, if that thread has one open.
struct RouteToThreadSink;

impl<S: Subscriber> Layer<S> for RouteToThreadSink {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        SINK.with(|sink| {
            // Checked before the visitor runs: an event from a thread with no
            // capture open costs one thread-local read.
            let Ok(sink) = sink.try_borrow() else {
                return;
            };
            let Some(logs) = sink.as_ref() else {
                return;
            };
            let mut visitor = LineVisitor(event.metadata().level().to_string());
            event.record(&mut visitor);
            logs.0.lock().expect("captured logs mutex").push(visitor.0);
        });
    }
}

/// Restores the sink this thread had before, so a nested capture and a panic
/// inside the body both leave the thread as they found it.
struct SinkGuard(Option<CapturedLogs>);

impl Drop for SinkGuard {
    fn drop(&mut self) {
        SINK.with(|sink| *sink.borrow_mut() = self.0.take());
    }
}

/// Installs the routing subscriber once for the whole test process.
fn install() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let subscriber = tracing_subscriber::registry().with(RouteToThreadSink);
        tracing::subscriber::set_global_default(subscriber)
            .expect("preview::log_capture needs the process default subscriber");
    });
}

/// Run `body` with every `tracing` event it emits on this thread captured.
pub fn capture<T>(body: impl FnOnce() -> T) -> (T, Vec<String>) {
    install();
    let logs = CapturedLogs::default();
    let previous = SINK.with(|sink| sink.borrow_mut().replace(logs.clone()));
    let _guard = SinkGuard(previous);
    let value = body();
    (value, logs.lines())
}

#[cfg(test)]
mod tests {
    use super::capture;

    /// One callsite both threads emit from, so the interest `tracing` caches
    /// for it is the same entry.
    fn emit(marker: &str) {
        tracing::warn!(marker, "log capture probe");
    }

    #[test]
    fn a_capture_hears_a_callsite_another_thread_registered_first() {
        let (_, logs) = capture(|| {
            // The other thread holds no capture, and it is the first to touch
            // the callsite. Under a thread-local subscriber that caches the
            // callsite as uninteresting and this capture goes deaf.
            std::thread::spawn(|| emit("elsewhere")).join().unwrap();
            emit("mine");
        });
        assert!(
            logs.iter().any(|line| line.contains("mine")),
            "logs={logs:?}"
        );
        // The other thread's event belongs to no capture and reaches none.
        assert!(
            !logs.iter().any(|line| line.contains("elsewhere")),
            "logs={logs:?}"
        );
    }

    #[test]
    fn concurrent_captures_keep_their_events_apart() {
        let other = std::thread::spawn(|| capture(|| emit("other-thread")).1);
        let (_, mine) = capture(|| {
            emit("this-thread");
            std::thread::yield_now();
            emit("this-thread-again");
        });
        let theirs = other.join().unwrap();
        assert_eq!(mine.len(), 2, "mine={mine:?}");
        assert!(mine.iter().all(|line| line.contains("this-thread")));
        assert_eq!(theirs.len(), 1, "theirs={theirs:?}");
        assert!(theirs[0].contains("other-thread"));
    }

    #[test]
    fn a_capture_restores_the_sink_it_replaced() {
        let (inner, outer) = capture(|| {
            let (_, inner) = capture(|| emit("inner"));
            emit("outer");
            inner
        });
        assert!(inner.iter().any(|line| line.contains("inner")));
        assert!(outer.iter().any(|line| line.contains("outer")));
        assert!(!outer.iter().any(|line| line.contains("inner")));
    }
}
