use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use writ_plugin::transform::builtins::register_builtins;
use writ_plugin::transform::TransformRegistry;

fn make_mixed_100kb() -> String {
    let chunks: &[&str] = &[
        "# Heading\n\n",
        "    fn example()   {\n",
        "        let x  =  42;\n",
        "        println!(\u{201c}value: {}\u{201d}, x);\n",
        "    }   \n",
        "\n",
        "Some prose with   extra spaces,and missing space before comma.\n",
        "Another line  \t  of text.\u{2019}s trailing spaces  \n",
        "\n",
    ];
    let target = 100 * 1024;
    let mut buf = String::with_capacity(target + 64);
    let mut idx = 0;
    while buf.len() < target {
        buf.push_str(chunks[idx % chunks.len()]);
        idx += 1;
    }
    buf
}

fn bench_transforms(c: &mut Criterion) {
    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).expect("register_builtins must succeed");

    let input = make_mixed_100kb();

    let mut group = c.benchmark_group("transforms_100kb");
    for descriptor in registry.list() {
        let id = descriptor.id.clone();
        let transform = registry.get(&id).expect("registered");
        group.bench_with_input(
            BenchmarkId::new("apply", &id),
            &input,
            |b, inp| {
                b.iter(|| {
                    transform.apply(inp).expect("transform must not fail");
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_transforms);
criterion_main!(benches);
