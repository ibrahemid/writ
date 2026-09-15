//! The one AI connection: which providers exist, and how each one's model
//! list is read.
//!
//! The provider table is data ([`providers`]), so the settings dropdown, the
//! localhost probe, the model fetch and the endpoint guard all read one
//! definition. The list parsers ([`models`]) are pure functions over a
//! response body, so every wire family is tested against a recorded answer
//! without a network. The connect flow's pure half ([`pkce`]) is the same
//! shape: the verifier, the challenge, the authorization URL and the callback
//! parser, with the socket left in the Tauri crate.

pub mod models;
pub mod pkce;
pub mod providers;
