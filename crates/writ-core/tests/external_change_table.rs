//! The Rust half of the shared external-change table.
//!
//! `src/services/__tests__/external-edit.fixture.test.ts` reads the same file
//! and asserts the frontend's route over it. The two decide the same question
//! in two languages — the backend cannot answer it alone, because whether a
//! document is unsaved is the editor's answer and nothing else's (ADR-033 §6)
//! — so the fixture is what keeps them from drifting apart.

use writ_core::notes::reload::{plan_reload, ReloadPlan};

struct Row {
    name: String,
    decided_by: String,
    rust: Option<(bool, bool, bool)>,
    plan: Option<ReloadPlan>,
}

fn fixture() -> Vec<Row> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/external-change-table.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture is readable");
    let raw: serde_json::Value = serde_json::from_str(&text).expect("fixture is json");
    raw["rows"]
        .as_array()
        .expect("rows is an array")
        .iter()
        .map(|row| Row {
            name: row["name"].as_str().expect("a name").to_string(),
            decided_by: row["decidedBy"].as_str().expect("decidedBy").to_string(),
            rust: row.get("rust").and_then(|inputs| {
                Some((
                    inputs["dirty"].as_bool()?,
                    inputs["changed"].as_bool()?,
                    inputs["removed"].as_bool()?,
                ))
            }),
            plan: row
                .get("plan")
                .and_then(|plan| serde_json::from_value(plan.clone()).ok()),
        })
        .collect()
}

#[test]
fn every_shared_row_plans_the_way_the_fixture_says() {
    for row in fixture() {
        let Some((dirty, changed, removed)) = row.rust else {
            continue;
        };
        let expected = row.plan.expect("a shared row names a plan");
        assert_eq!(
            plan_reload(dirty, changed, removed),
            expected,
            "{}",
            row.name
        );
    }
}

#[test]
fn every_row_is_either_shared_with_the_policy_or_the_editors_alone() {
    // A row with no `decidedBy` would pass both halves by being skipped in
    // both, which is the drift the fixture exists to stop.
    for row in fixture() {
        match row.decided_by.as_str() {
            "shared" => assert!(row.rust.is_some(), "{} names no inputs", row.name),
            "frontend" => assert!(row.rust.is_none(), "{} answers here too", row.name),
            other => panic!("{} is decided by {other}", row.name),
        }
    }
}

#[test]
fn the_shared_rows_reach_all_three_plans() {
    let plans: Vec<ReloadPlan> = fixture().into_iter().filter_map(|row| row.plan).collect();
    for plan in [
        ReloadPlan::ReplaceQuietly,
        ReloadPlan::Ask,
        ReloadPlan::Ignore,
    ] {
        assert!(plans.contains(&plan), "no row reaches {plan:?}");
    }
}

#[test]
fn the_table_holds_every_row_both_halves_read() {
    // Both halves assert this count. A row added to one side and not the
    // other fails here first, with the number rather than a missing case.
    assert_eq!(fixture().len(), 12);
}
