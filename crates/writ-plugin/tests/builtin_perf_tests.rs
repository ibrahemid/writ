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

#[test]
fn each_builtin_runs_under_perf_budget_on_100kb_input() {
    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).unwrap();
    let input = make_input_100kb();
    for descriptor in registry.list() {
        let t = registry.get(&descriptor.id).expect("present");
        let start = Instant::now();
        let _ = t.apply(&input).expect("transform must succeed");
        let elapsed = start.elapsed().as_millis();
        assert!(
            elapsed < PERF_BUDGET_MS,
            "{} took {}ms (budget {}ms)",
            descriptor.id,
            elapsed,
            PERF_BUDGET_MS,
        );
    }
}
