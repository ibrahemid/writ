//! Domain events and an in-process event bus.
//!
//! Events are the primary integration point between the core and the
//! rest of the application. The host subscribes to the bus and forwards
//! events to Tauri listeners, plugins, or tests.

/// In-process event bus and [`bus::WritEvent`] payload types.
pub mod bus;
