use std::time::Instant;

use writ_plugin::transform::builtins::register_builtins;
use writ_plugin::transform::TransformRegistry;

const PERF_BUDGET_MS: u128 = 50;

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

#[test]
fn every_builtin_transforms_a_100kb_input() {
    let registry = registry_with_builtins();
    let input = make_input_100kb();
    let descriptors = registry.list();
    assert!(!descriptors.is_empty(), "no builtins registered");
    for descriptor in descriptors {
        let t = registry.get(&descriptor.id).expect("present");
        let out = t.apply(&input).expect("transform must succeed");
        assert!(!out.is_empty(), "{} emptied a 100KB input", descriptor.id);
    }
}

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
