use std::time::Instant;

use writ_plugin::transform::builtins::register_builtins;
use writ_plugin::transform::TransformRegistry;

const FIXTURE_TARGET_BYTES: usize = 100 * 1024;
const MEDIAN_SAMPLES: usize = 9;
const BUDGET_MS: u128 = 100;

fn build_fixture() -> String {
    let chunks: &[&str] = &[
        "# Heading one\n\n",
        "    fn example()   {\n",
        "        let x  =  42;\n",
        "        println!(\u{201c}value: {}\u{201d}, x);\n",
        "    }   \n",
        "\n",
        "Some prose with   extra spaces,and missing space before comma.\n",
        "Another line  \t  of text.\u{2019}s trailing spaces  \n",
        "\n",
    ];
    let mut buf = String::with_capacity(FIXTURE_TARGET_BYTES + 64);
    let mut idx = 0;
    while buf.len() < FIXTURE_TARGET_BYTES {
        buf.push_str(chunks[idx % chunks.len()]);
        idx += 1;
    }
    buf
}

fn median_elapsed_ms(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn fixture_is_at_least_100kb() {
    assert!(
        build_fixture().len() >= FIXTURE_TARGET_BYTES,
        "fixture must be >= {} bytes",
        FIXTURE_TARGET_BYTES,
    );
}

#[test]
fn fixture_is_deterministic() {
    let a = build_fixture();
    let b = build_fixture();
    assert_eq!(a, b, "fixture must be identical across two calls");
}

#[test]
fn transform_budget_100kb() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).expect("register_builtins");
    let input = build_fixture();

    for descriptor in registry.list() {
        let transform = registry.get(&descriptor.id).expect("present");
        let mut samples = Vec::with_capacity(MEDIAN_SAMPLES);
        for _ in 0..MEDIAN_SAMPLES {
            let start = Instant::now();
            transform.apply(&input).expect("transform must not fail");
            samples.push(start.elapsed().as_millis());
        }
        let median = median_elapsed_ms(samples);
        assert!(
            median < BUDGET_MS,
            "transform '{}' median {}ms exceeds budget {}ms on 100KB input",
            descriptor.id,
            median,
            BUDGET_MS,
        );
    }
}
