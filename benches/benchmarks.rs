use criterion::{criterion_group, criterion_main, Criterion};

pub mod common;
use common::*;

fn benchmarks(c: &mut Criterion) {
    c.bench_function("simple_insert", |b| {
        let mut bench = simple_insert::Benchmark::new();
        b.iter(|| bench.run())
    })
    .bench_function("add_remove", |b| {
        let mut bench = add_remove::Benchmark::new();
        b.iter(|| bench.run())
    });

    c.benchmark_group("frag_iter")
        .bench_function("for", |b| {
            let mut bench = frag_iter::Benchmark::new();
            b.iter(|| bench.run())
        })
        .bench_function("for2", |b| {
            let mut bench = frag_iter::Benchmark::new();
            b.iter(|| bench.run2())
        })
        .bench_function("for_each", |b| {
            let mut bench = frag_iter::Benchmark::new();
            b.iter(|| bench.run_for_each())
        })
        .bench_function("for_each2", |b| {
            let mut bench = frag_iter::Benchmark::new();
            b.iter(|| bench.run_for_each2())
        });

    c.benchmark_group("simple_iter")
        .bench_function("iter", |b| {
            let mut bench = simple_iter::Benchmark::new();
            b.iter(|| bench.run())
        })
        .bench_function("manual_flatten", |b| {
            let mut bench = simple_iter::Benchmark::new();
            b.iter(|| bench.run_manual_flatten())
        });

    c.benchmark_group("heavy_compute")
        .bench_function("par", |b| {
            let mut bench = heavy_compute::Benchmark::new();
            b.iter(|| bench.run())
        })
        .bench_function("seq", |b| {
            let mut bench = heavy_compute::Benchmark::new();
            b.iter(|| bench.run_seq())
        });

    c.benchmark_group("schedule")
        .bench_function("inner_par", |b| {
            let mut bench = schedule_inner_par::Benchmark::new();
            b.iter(|| bench.run())
        })
        .bench_function("par", |b| {
            let mut bench = schedule::Benchmark::new();
            b.iter(|| bench.run())
        })
        .bench_function("seq", |b| {
            let mut bench = schedule::Benchmark::new();
            b.iter(|| bench.run_seq())
        });

    #[cfg(feature = "serde")]
    c.benchmark_group("benchmark")
        .bench_function("binary_row", |b| {
            let mut bench = serialize_binary::Benchmark::new();
            b.iter(|| bench.run_row())
        })
        .bench_function("binary_col", |b| {
            let mut bench = serialize_binary::Benchmark::new();
            b.iter(|| bench.run_col())
        })
        .bench_function("text_row", |b| {
            let mut bench = serialize_text::Benchmark::new();
            b.iter(|| bench.run_row())
        })
        .bench_function("text_col", |b| {
            let mut bench = serialize_text::Benchmark::new();
            b.iter(|| bench.run_col())
        });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
