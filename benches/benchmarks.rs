use criterion::{criterion_group, criterion_main, Criterion};

pub mod common;
use common::*;

fn benchmarks(c: &mut Criterion) {
    c.bench_function("simple_insert", |b| {
        let mut bench = simple_insert::Benchmark::new();
        b.iter(|| bench.run())
    })
    .bench_function("simple_iter", |b| {
        let mut bench = simple_iter::Benchmark::new();
        b.iter(|| bench.run())
    })
    .bench_function("frag_iter", |b| {
        let mut bench = frag_iter::Benchmark::new();
        b.iter(|| bench.run())
    })
    .bench_function("heavy_compute", |b| {
        let mut bench = heavy_compute::Benchmark::new();
        b.iter(|| bench.run())
    });

    c.benchmark_group("add_remove")
        .bench_function("current", |b| {
            let mut bench = add_remove::Benchmark::new();
            b.iter(|| bench.run())
        })
        .bench_function("alt", |b| {
            let mut bench = add_remove::Benchmark::new();
            b.iter(|| bench.run_alt())
        });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
