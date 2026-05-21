use writ_core::update::{UpdateEvent, UpdatePhase};

#[test]
fn idle_starts_checking() {
    let next = UpdatePhase::Idle.apply(UpdateEvent::CheckStarted).unwrap();
    assert_eq!(next, UpdatePhase::Checking);
}

#[test]
fn checking_found_update_carries_version() {
    let next = UpdatePhase::Checking
        .apply(UpdateEvent::UpdateFound {
            version: "0.9.0".to_string(),
        })
        .unwrap();
    assert_eq!(
        next,
        UpdatePhase::Available {
            version: "0.9.0".to_string()
        }
    );
}

#[test]
fn checking_no_update_is_up_to_date() {
    let next = UpdatePhase::Checking.apply(UpdateEvent::NoUpdate).unwrap();
    assert_eq!(next, UpdatePhase::UpToDate);
}

#[test]
fn checking_error_carries_message_to_failed() {
    let next = UpdatePhase::Checking
        .apply(UpdateEvent::Errored {
            message: "network down".to_string(),
        })
        .unwrap();
    assert_eq!(
        next,
        UpdatePhase::Failed {
            message: "network down".to_string()
        }
    );
}

#[test]
fn available_starts_download_with_zeroed_progress() {
    let next = UpdatePhase::Available {
        version: "0.9.0".to_string(),
    }
    .apply(UpdateEvent::DownloadStarted { total: Some(2048) })
    .unwrap();
    assert_eq!(
        next,
        UpdatePhase::Downloading {
            downloaded: 0,
            total: Some(2048)
        }
    );
}

#[test]
fn download_progress_reports_absolute_bytes_and_total() {
    let phase = UpdatePhase::Downloading {
        downloaded: 100,
        total: None,
    };
    let next = phase
        .apply(UpdateEvent::DownloadProgressed {
            downloaded: 350,
            total: Some(2048),
        })
        .unwrap();
    assert_eq!(
        next,
        UpdatePhase::Downloading {
            downloaded: 350,
            total: Some(2048)
        }
    );
}

#[test]
fn download_completed_moves_to_installing() {
    let next = UpdatePhase::Downloading {
        downloaded: 2048,
        total: Some(2048),
    }
    .apply(UpdateEvent::DownloadCompleted)
    .unwrap();
    assert_eq!(next, UpdatePhase::Installing);
}

#[test]
fn install_completed_is_ready() {
    let next = UpdatePhase::Installing
        .apply(UpdateEvent::InstallCompleted)
        .unwrap();
    assert_eq!(next, UpdatePhase::Ready);
}

#[test]
fn download_error_moves_to_failed() {
    let next = UpdatePhase::Downloading {
        downloaded: 100,
        total: None,
    }
    .apply(UpdateEvent::Errored {
        message: "checksum mismatch".to_string(),
    })
    .unwrap();
    assert_eq!(
        next,
        UpdatePhase::Failed {
            message: "checksum mismatch".to_string()
        }
    );
}

#[test]
fn up_to_date_can_recheck() {
    let next = UpdatePhase::UpToDate
        .apply(UpdateEvent::CheckStarted)
        .unwrap();
    assert_eq!(next, UpdatePhase::Checking);
}

#[test]
fn failed_can_retry_check() {
    let next = UpdatePhase::Failed {
        message: "boom".to_string(),
    }
    .apply(UpdateEvent::CheckStarted)
    .unwrap();
    assert_eq!(next, UpdatePhase::Checking);
}

#[test]
fn available_can_be_dismissed_to_idle() {
    let next = UpdatePhase::Available {
        version: "0.9.0".to_string(),
    }
    .apply(UpdateEvent::Dismissed)
    .unwrap();
    assert_eq!(next, UpdatePhase::Idle);
}

#[test]
fn idle_cannot_skip_to_downloading() {
    let err = UpdatePhase::Idle
        .apply(UpdateEvent::DownloadStarted { total: None })
        .unwrap_err();
    assert_eq!(err.from, "idle");
    assert_eq!(err.event, "download_started");
}

#[test]
fn checking_cannot_skip_download_to_installing() {
    let result = UpdatePhase::Checking.apply(UpdateEvent::DownloadCompleted);
    assert!(result.is_err());
}

#[test]
fn ready_rejects_recheck_until_restart() {
    let result = UpdatePhase::Ready.apply(UpdateEvent::CheckStarted);
    assert!(result.is_err());
}

#[test]
fn ready_can_be_dismissed_after_install() {
    let next = UpdatePhase::Ready.apply(UpdateEvent::Dismissed).unwrap();
    assert_eq!(next, UpdatePhase::Idle);
}

#[test]
fn downloading_cannot_be_dismissed_mid_flight() {
    let result = UpdatePhase::Downloading {
        downloaded: 10,
        total: Some(100),
    }
    .apply(UpdateEvent::Dismissed);
    assert!(result.is_err());
}

#[test]
fn available_phase_serializes_with_tag_and_version() {
    let json = serde_json::to_value(UpdatePhase::Available {
        version: "0.9.0".to_string(),
    })
    .unwrap();
    assert_eq!(json["status"], "available");
    assert_eq!(json["version"], "0.9.0");
}

#[test]
fn downloading_phase_serializes_progress_fields() {
    let json = serde_json::to_value(UpdatePhase::Downloading {
        downloaded: 350,
        total: Some(2048),
    })
    .unwrap();
    assert_eq!(json["status"], "downloading");
    assert_eq!(json["downloaded"], 350);
    assert_eq!(json["total"], 2048);
}

#[test]
fn idle_phase_serializes_as_bare_status() {
    let json = serde_json::to_value(UpdatePhase::Idle).unwrap();
    assert_eq!(json["status"], "idle");
}
