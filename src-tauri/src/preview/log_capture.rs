//! Test-only capture of `tracing` events, so a log line a rule depends on
//! can be asserted rather than assumed.
//!
//! Uses the `tracing-subscriber` the app already logs through: a layer that
//! keeps each event as a line, installed for the current thread only, so
//! tests running in parallel do not see each other's events.

use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

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

impl<S: Subscriber> Layer<S> for CapturedLogs {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = LineVisitor(event.metadata().level().to_string());
        event.record(&mut visitor);
        self.0.lock().expect("captured logs mutex").push(visitor.0);
    }
}

/// Run `body` with every `tracing` event it emits captured.
pub fn capture<T>(body: impl FnOnce() -> T) -> (T, Vec<String>) {
    let logs = CapturedLogs::default();
    let subscriber = tracing_subscriber::registry().with(logs.clone());
    let value = tracing::subscriber::with_default(subscriber, body);
    (value, logs.lines())
}
