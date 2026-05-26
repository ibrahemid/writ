use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::{LayoutStateRecord, LayoutStateStore};

fn setup() -> (TempDir, LayoutStateStore) {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    (dir, LayoutStateStore::new(conn))
}

fn record(path: &str) -> LayoutStateRecord {
    LayoutStateRecord {
        path: path.to_string(),
        layout_mode: "split".to_string(),
        split_ratio: Some(0.6),
        last_view_mode: "preview".to_string(),
    }
}

#[test]
fn get_returns_none_for_unknown_path() {
    let (_dir, store) = setup();
    assert_eq!(store.get("/no/such/path").unwrap(), None);
}

#[test]
fn set_then_get_round_trips() {
    let (_dir, store) = setup();
    let rec = record("/home/user/page.html");
    store.set(&rec).unwrap();
    let got = store.get("/home/user/page.html").unwrap().unwrap();
    assert_eq!(got, rec);
}

#[test]
fn set_is_an_upsert() {
    let (_dir, store) = setup();
    store.set(&record("/p.html")).unwrap();

    let updated = LayoutStateRecord {
        path: "/p.html".to_string(),
        layout_mode: "preview".to_string(),
        split_ratio: None,
        last_view_mode: "preview".to_string(),
    };
    store.set(&updated).unwrap();

    let got = store.get("/p.html").unwrap().unwrap();
    assert_eq!(got.layout_mode, "preview");
    assert_eq!(got.split_ratio, None);
}

#[test]
fn null_split_ratio_round_trips() {
    let (_dir, store) = setup();
    let rec = LayoutStateRecord {
        path: "/src.rs".to_string(),
        layout_mode: "source".to_string(),
        split_ratio: None,
        last_view_mode: "source".to_string(),
    };
    store.set(&rec).unwrap();
    assert_eq!(store.get("/src.rs").unwrap().unwrap().split_ratio, None);
}

#[test]
fn remove_deletes_the_row() {
    let (_dir, store) = setup();
    store.set(&record("/gone.html")).unwrap();
    store.remove("/gone.html").unwrap();
    assert_eq!(store.get("/gone.html").unwrap(), None);
}

#[test]
fn invalid_layout_mode_is_rejected_by_check_constraint() {
    let (_dir, store) = setup();
    let bad = LayoutStateRecord {
        path: "/x.html".to_string(),
        layout_mode: "garbage".to_string(),
        split_ratio: None,
        last_view_mode: "source".to_string(),
    };
    // The CHECK constraint on layout_mode rejects unknown discriminants.
    assert!(store.set(&bad).is_err());
}
