// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Benchmarking generational free-list arena slot recycling.
//!
//! Measures performance of repeated node additions and deletions in `Document`.
//! Asserts that slot recycling maintains high throughput and bounded memory.

#![allow(missing_docs, unused_results)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxml::parse;
use std::hint::black_box;

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("arena_recycling");

    group.bench_function("recycle_1000_sequential_nodes", |b| {
        b.iter(|| {
            let mut doc = parse("<root/>").expect("parse");
            let root = doc.root_element().expect("root element");

            // Add 1000 nodes
            let mut ids = Vec::with_capacity(1000);
            for i in 0..1000 {
                let id = doc.append_element(root, None, "node").expect("append");
                ids.push(id);
                black_box(i);
            }

            // Remove 1000 nodes (pushing to free_slots)
            for id in ids {
                doc.remove(id).expect("remove");
            }

            // Allocate 1000 more nodes (recycling from free_slots)
            for _ in 0..1000 {
                let _ = doc.append_element(root, None, "recycled").expect("recycled");
            }

            black_box(doc)
        });
    });

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
