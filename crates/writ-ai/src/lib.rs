//! The AI connection of the Writ editor, with no Tauri in it.
//!
//! [`keys`] keeps provider keys, [`connection`] holds the HTTP client, the
//! stream reader, model lists, probes and Rewrite, and [`chat`] the chat pane's
//! request, stream, context and apply. The app shell's `commands::ai` and
//! `commands::chat` are wrappers over these, so another binary can reach the
//! same connection without linking Tauri.

pub mod chat;
pub mod connection;
pub mod keys;
#[cfg(test)]
mod log_capture;
mod poison;

pub use connection::AiState;
pub use keys::{AiKeyState, KeyStore, MemoryKeyStore};
