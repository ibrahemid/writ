//! Update lifecycle policy.
//!
//! This module owns the *policy* for the in-app updater: the legal set of
//! phases an update can be in and the transitions between them. It has no
//! knowledge of the network, the Tauri updater plugin, or how bytes are
//! downloaded. The `writ-tauri` crate drives the *mechanism* (checking,
//! downloading, installing) and feeds observed [`UpdateEvent`]s into
//! [`UpdatePhase::apply`], which rejects any illegal transition.
//!
//! [`UpdatePhase`] is the single source of truth for update state. It
//! serializes to a `status`-tagged object so the frontend can mirror it
//! directly without a parallel state model.

use serde::Serialize;
use thiserror::Error;

/// A phase in the update lifecycle.
///
/// Serializes with an internal `status` tag, e.g.
/// `{"status":"available","version":"0.9.0"}` or
/// `{"status":"downloading","downloaded":350,"total":2048}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdatePhase {
    /// No update activity. Initial and dismissed state.
    #[default]
    Idle,
    /// A check is in flight.
    Checking,
    /// A check completed and the running version is current.
    UpToDate,
    /// A newer version is available and awaiting user action.
    Available {
        /// Semver of the available update.
        version: String,
    },
    /// The update bundle is downloading.
    Downloading {
        /// Bytes downloaded so far.
        downloaded: u64,
        /// Total bytes expected, when the server reported a length.
        total: Option<u64>,
    },
    /// The bundle finished downloading and is being installed.
    Installing,
    /// The update is installed and the app is awaiting a restart.
    Ready,
    /// The update flow failed. Carries a sanitized, user-safe message.
    Failed {
        /// Human-readable, secret-free description of the failure.
        message: String,
    },
}

/// An observed event that drives an [`UpdatePhase`] transition.
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    /// A check was initiated.
    CheckStarted,
    /// A check found a newer version.
    UpdateFound {
        /// Semver of the discovered update.
        version: String,
    },
    /// A check found no newer version.
    NoUpdate,
    /// A download began. `total` is the content length, when known.
    DownloadStarted {
        /// Total bytes expected, when the server reported a length.
        total: Option<u64>,
    },
    /// Download progress was observed.
    DownloadProgressed {
        /// Total bytes downloaded so far.
        downloaded: u64,
        /// Total bytes expected, when the server reported a length.
        total: Option<u64>,
    },
    /// The download completed.
    DownloadCompleted,
    /// Installation completed; the app is staged for restart.
    InstallCompleted,
    /// The flow failed with the given sanitized message.
    Errored {
        /// Sanitized, user-safe description of the failure.
        message: String,
    },
    /// The user dismissed the update surface.
    Dismissed,
}

/// Error returned when an [`UpdateEvent`] is not valid from the current
/// [`UpdatePhase`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("illegal update transition: '{event}' is not valid from '{from}'")]
pub struct IllegalTransition {
    /// Label of the phase the transition was attempted from.
    pub from: &'static str,
    /// Label of the event that was rejected.
    pub event: &'static str,
}

impl UpdatePhase {
    /// Applies an [`UpdateEvent`], returning the next phase or an
    /// [`IllegalTransition`] if the event is not legal from `self`.
    pub fn apply(&self, event: UpdateEvent) -> Result<UpdatePhase, IllegalTransition> {
        use UpdateEvent as E;
        use UpdatePhase as P;

        let from = self.label();
        let attempted = event.label();

        let next = match (self, event) {
            (P::Idle | P::UpToDate | P::Available { .. } | P::Failed { .. }, E::CheckStarted) => {
                P::Checking
            }

            (P::Checking, E::UpdateFound { version }) => P::Available { version },
            (P::Checking, E::NoUpdate) => P::UpToDate,

            (P::Available { .. }, E::DownloadStarted { total }) => P::Downloading {
                downloaded: 0,
                total,
            },

            (P::Downloading { .. }, E::DownloadProgressed { downloaded, total }) => {
                P::Downloading { downloaded, total }
            }
            (P::Downloading { .. }, E::DownloadCompleted) => P::Installing,

            (P::Installing, E::InstallCompleted) => P::Ready,

            (
                P::Checking | P::Available { .. } | P::Downloading { .. } | P::Installing,
                E::Errored { message },
            ) => P::Failed { message },

            (
                P::Checking | P::UpToDate | P::Available { .. } | P::Ready | P::Failed { .. },
                E::Dismissed,
            ) => P::Idle,

            _ => {
                return Err(IllegalTransition {
                    from,
                    event: attempted,
                })
            }
        };

        Ok(next)
    }

    fn label(&self) -> &'static str {
        match self {
            UpdatePhase::Idle => "idle",
            UpdatePhase::Checking => "checking",
            UpdatePhase::UpToDate => "up_to_date",
            UpdatePhase::Available { .. } => "available",
            UpdatePhase::Downloading { .. } => "downloading",
            UpdatePhase::Installing => "installing",
            UpdatePhase::Ready => "ready",
            UpdatePhase::Failed { .. } => "failed",
        }
    }
}

impl UpdateEvent {
    fn label(&self) -> &'static str {
        match self {
            UpdateEvent::CheckStarted => "check_started",
            UpdateEvent::UpdateFound { .. } => "update_found",
            UpdateEvent::NoUpdate => "no_update",
            UpdateEvent::DownloadStarted { .. } => "download_started",
            UpdateEvent::DownloadProgressed { .. } => "download_progressed",
            UpdateEvent::DownloadCompleted => "download_completed",
            UpdateEvent::InstallCompleted => "install_completed",
            UpdateEvent::Errored { .. } => "errored",
            UpdateEvent::Dismissed => "dismissed",
        }
    }
}
