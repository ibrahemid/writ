//! The one AI connection: which providers exist, and how each one's model
//! list is read.
//!
//! The provider table is data ([`providers`]), so the settings dropdown, the
//! localhost probe, the model fetch and the endpoint guard all read one
//! definition. The list parsers ([`models`]) are pure functions over a
//! response body, so every wire family is tested against a recorded answer
//! without a network.

pub mod providers;
