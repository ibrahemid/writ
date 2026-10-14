use writ_core::update::auto_check::{
    last_known_check, next_silent_check_step, SilentCheckStep, MIN_CHECK_INTERVAL_MS,
    RESIDENT_WAKE_INTERVAL_MS,
};
use writ_core::update::install::{is_managed, InstallFacts};

const HOUR: u64 = 60 * 60 * 1000;
const DAY: u64 = MIN_CHECK_INTERVAL_MS;

fn appimage_in_home() -> InstallFacts {
    InstallFacts {
        package_manager_marker: false,
        replaces_in_place: true,
        appimage_env: true,
        target_writable: true,
    }
}

#[test]
fn an_appimage_the_user_owns_updates_itself() {
    assert!(!is_managed(&appimage_in_home()));
}

#[test]
fn a_package_manager_marker_makes_any_install_managed() {
    for replaces_in_place in [true, false] {
        let facts = InstallFacts {
            package_manager_marker: true,
            replaces_in_place,
            ..appimage_in_home()
        };
        assert!(is_managed(&facts), "replaces_in_place={replaces_in_place}");
    }
}

#[test]
fn an_unpacked_appimage_without_appimage_set_is_managed() {
    let facts = InstallFacts {
        appimage_env: false,
        ..appimage_in_home()
    };
    assert!(is_managed(&facts));
}

#[test]
fn an_appimage_in_a_directory_the_user_cannot_write_is_managed() {
    let facts = InstallFacts {
        target_writable: false,
        ..appimage_in_home()
    };
    assert!(is_managed(&facts));
}

#[test]
fn an_installer_that_elevates_is_not_managed_by_ownership_alone() {
    let facts = InstallFacts {
        package_manager_marker: false,
        replaces_in_place: false,
        appimage_env: false,
        target_writable: false,
    };
    assert!(!is_managed(&facts));
}

/// Runs the resident checker against a simulated clock: a check takes
/// `check_ms`, a wait advances the clock by what it asked for plus
/// `oversleep_ms` (a suspended machine). Returns the times checks started.
fn simulate(
    start_ms: u64,
    last_check_ms: Option<u64>,
    run_for_ms: u64,
    auto_check: impl Fn(u64) -> bool,
    oversleep_ms: impl Fn(u64) -> u64,
) -> Vec<u64> {
    let mut now = start_ms;
    let mut last = last_check_ms;
    let mut checks = Vec::new();
    while now < start_ms + run_for_ms {
        match next_silent_check_step(auto_check(now), last, now) {
            SilentCheckStep::CheckNow => {
                checks.push(now);
                last = Some(now);
                now += 1_000;
            }
            SilentCheckStep::Wait(ms) => {
                assert!(ms > 0 && ms <= RESIDENT_WAKE_INTERVAL_MS, "wait of {ms} ms");
                now += ms + oversleep_ms(now);
            }
        }
    }
    checks
}

#[test]
fn a_resident_writ_checks_again_each_day() {
    let launch = 1_000 * DAY;
    // The last check ran two hours before this launch.
    let checks = simulate(launch, Some(launch - 2 * HOUR), 3 * DAY, |_| true, |_| 0);
    assert_eq!(
        checks,
        vec![launch + 22 * HOUR, launch + 46 * HOUR, launch + 70 * HOUR]
    );
}

#[test]
fn a_first_launch_checks_at_once_and_again_after_a_day_of_uptime() {
    let launch = 1_000 * DAY;
    let checks = simulate(launch, None, DAY + HOUR, |_| true, |_| 0);
    assert_eq!(checks, vec![launch, launch + DAY]);
}

#[test]
fn turning_the_check_off_stops_it_and_turning_it_on_resumes_within_an_hour() {
    let launch = 1_000 * DAY;
    // Off from hour 1 to hour 40.
    let on = move |now: u64| now < launch + HOUR || now >= launch + 40 * HOUR;
    let checks = simulate(launch, None, 2 * DAY, on, |_| 0);
    assert_eq!(checks.len(), 2, "{checks:?}");
    assert_eq!(checks[0], launch);
    assert!(
        checks[1] >= launch + 40 * HOUR && checks[1] <= launch + 41 * HOUR,
        "{checks:?}"
    );
}

#[test]
fn a_machine_that_slept_past_the_due_time_checks_on_its_next_look() {
    let launch = 1_000 * DAY;
    // The machine sleeps for 30 hours during the first wait.
    let slept = std::cell::Cell::new(false);
    let checks = simulate(
        launch,
        Some(launch),
        2 * DAY,
        |_| true,
        |_| {
            if slept.replace(true) {
                0
            } else {
                30 * HOUR
            }
        },
    );
    assert_eq!(checks.first(), Some(&(launch + HOUR + 30 * HOUR)));
}

#[test]
fn a_data_directory_that_refuses_the_timestamp_still_checks_once_a_day() {
    let launch = 1_000 * DAY;
    let recorded: Option<u64> = None;
    let mut ran = None;
    let mut now = launch;
    let mut checks = Vec::new();
    while now < launch + 2 * DAY {
        match next_silent_check_step(true, last_known_check(recorded, ran), now) {
            SilentCheckStep::CheckNow => {
                checks.push(now);
                ran = Some(now);
                now += 1_000;
            }
            SilentCheckStep::Wait(ms) => now += ms,
        }
    }
    assert_eq!(checks, vec![launch, launch + DAY]);
    assert_eq!(last_known_check(Some(5), Some(3)), Some(5));
    assert_eq!(last_known_check(Some(3), Some(5)), Some(5));
}
