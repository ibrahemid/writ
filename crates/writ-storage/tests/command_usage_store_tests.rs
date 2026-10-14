use std::collections::{HashMap, HashSet};

use tempfile::TempDir;
use writ_core::command::usage::{CommandUsage, CommandUse};
use writ_storage::command_usage::CommandUsageStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

fn setup() -> (TempDir, CommandUsageStore) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    let store = CommandUsageStore::new(open_database(&db_path).expect("second connection"));
    (dir, store)
}

fn used(id: &str, count: u32, last_used_ms: u64) -> CommandUse {
    CommandUse {
        id: id.to_string(),
        count,
        last_used_ms,
    }
}

fn usage(count: u32, last_used_ms: u64) -> CommandUsage {
    CommandUsage {
        count,
        last_used_ms,
    }
}

#[test]
fn a_fresh_database_holds_no_usage() {
    let (_dir, store) = setup();
    assert!(store.all().unwrap().is_empty());
}

#[test]
fn recorded_runs_add_up_and_the_time_only_moves_forward() {
    let (_dir, store) = setup();
    store
        .record(&[used("palette.open", 1, 100), used("tab.new", 3, 50)])
        .unwrap();
    store.record(&[used("palette.open", 2, 90)]).unwrap();

    let all = store.all().unwrap();
    assert_eq!(all["palette.open"], usage(3, 100));
    assert_eq!(all["tab.new"], usage(3, 50));
}

#[test]
fn clear_forgets_every_command() {
    let (_dir, store) = setup();
    store.record(&[used("a", 1, 1), used("b", 1, 1)]).unwrap();
    store.clear().unwrap();
    assert!(store.all().unwrap().is_empty());
}

#[test]
fn prune_drops_only_commands_that_no_longer_exist() {
    let (_dir, store) = setup();
    store
        .record(&[used("kept", 1, 1), used("gone", 4, 4)])
        .unwrap();
    let known: HashSet<String> = ["kept".to_string()].into_iter().collect();

    assert_eq!(store.prune(&known).unwrap(), 1);
    let all = store.all().unwrap();
    assert_eq!(all.len(), 1);
    assert!(all.contains_key("kept"));
}

#[test]
fn the_config_s_usage_is_imported_once() {
    let (_dir, store) = setup();
    let from_config: HashMap<String, CommandUsage> =
        [("palette.open".to_string(), usage(17, 1_715_500_000_000))]
            .into_iter()
            .collect();

    assert!(store.import_once(&from_config).unwrap());
    assert_eq!(
        store.all().unwrap()["palette.open"],
        usage(17, 1_715_500_000_000)
    );

    // A later launch whose file still carries the table moves nothing, and a
    // clear is not undone by it.
    store.clear().unwrap();
    assert!(!store.import_once(&from_config).unwrap());
    assert!(store.all().unwrap().is_empty());
}

#[test]
fn an_import_never_counts_a_run_twice() {
    let (_dir, store) = setup();
    store.record(&[used("palette.open", 20, 5)]).unwrap();
    let from_config: HashMap<String, CommandUsage> = [("palette.open".to_string(), usage(17, 9))]
        .into_iter()
        .collect();

    store.import_once(&from_config).unwrap();
    assert_eq!(store.all().unwrap()["palette.open"], usage(20, 9));
}
