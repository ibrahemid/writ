use std::time::Instant;

use writ_plugin::transform::builtins::register_builtins;
use writ_plugin::transform::TransformRegistry;

const PERF_BUDGET_MS: u128 = 50;

/// Builtins that `builtin_transforms_tests.rs` pins as idempotent (every one
/// except `prepare_prompt`, which has no such expectation).
const IDEMPOTENT: &[&str] = &[
    "dedent",
    "ensure_final_newline",
    "fix_punctuation_spacing",
    "normalize_whitespace",
    "smart_to_straight_quotes",
    "tidy_whitespace",
    "trim_leading_whitespace",
    "trim_trailing_whitespace",
];

fn make_input_100kb() -> String {
    let line = "    \"hello   world\u{2019}s test\"\n";
    let target = 100 * 1024;
    let mut buf = String::with_capacity(target + line.len());
    while buf.len() < target {
        buf.push_str(line);
    }
    buf
}

fn registry_with_builtins() -> TransformRegistry {
    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).unwrap();
    registry
}

fn has_trailing_space(text: &str) -> bool {
    text.lines()
        .any(|line| line.ends_with(' ') || line.ends_with('\t'))
}

#[test]
fn every_builtin_holds_its_contract_on_a_100kb_input() {
    let registry = registry_with_builtins();
    let input = make_input_100kb();
    let descriptors = registry.list();
    assert_eq!(descriptors.len(), 9, "every builtin must be covered");

    for descriptor in descriptors {
        let id = descriptor.id.as_str();
        let t = registry.get(id).expect("present");
        let out = t.apply(&input).expect("transform must succeed");
        assert!(!out.is_empty(), "{id} emptied a 100KB input");
        assert_eq!(
            out,
            t.apply(&input).expect("transform must succeed"),
            "{id} is not deterministic on the same input"
        );
        if IDEMPOTENT.contains(&id) {
            assert_eq!(
                t.apply(&out).expect("transform must succeed"),
                out,
                "{id} is not idempotent on a 100KB input"
            );
        }
        match id {
            "trim_trailing_whitespace" | "tidy_whitespace" => {
                assert!(!has_trailing_space(&out), "{id} left a trailing space");
            }
            _ => {}
        }
        match id {
            "ensure_final_newline" | "tidy_whitespace" => {
                assert!(out.ends_with('\n'), "{id} dropped the final newline");
                assert!(
                    !out.ends_with("\n\n"),
                    "{id} left more than one final newline"
                );
            }
            _ => {}
        }
    }
}

/// Wall-clock budget for the same input. Timing under a loaded machine says
/// nothing about the code, so this runs only when asked for: from
/// `scripts/perf-gate.sh`, or by hand with
/// `cargo test --release -p writ-plugin --test builtin_perf_tests -- --ignored --nocapture`.
#[test]
#[ignore = "timing probe; run with --release --ignored --nocapture"]
fn timing_100kb_builtins() {
    let registry = registry_with_builtins();
    let input = make_input_100kb();
    for descriptor in registry.list() {
        let t = registry.get(&descriptor.id).expect("present");
        // Warm any lazy statics the transform builds on first use.
        let _ = t.apply(&input).expect("transform must succeed");
        let start = Instant::now();
        let _ = t.apply(&input).expect("transform must succeed");
        let elapsed = start.elapsed();
        println!("{}: {:?} on {} bytes", descriptor.id, elapsed, input.len());
        assert!(
            elapsed.as_millis() < PERF_BUDGET_MS,
            "{} took {}ms (budget {}ms)",
            descriptor.id,
            elapsed.as_millis(),
            PERF_BUDGET_MS,
        );
    }
}
