// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Benchmarking the mutable tree: slot recycling, block growth, and
//! the reads that mutation changes.
//!
//! Measures repeated node additions and deletions in `Document`, the
//! cost of appending into one block, and document-order queries on a
//! mutated tree.

#![allow(missing_docs, unused_results)]

use core::fmt::Write as _;
use criterion::{Criterion, criterion_group, criterion_main};
use oxml::parse;
use std::hint::black_box;

fn document(n: usize) -> String {
    let mut s = String::from("<root xmlns:m=\"urn:m\">");
    for i in 0..n {
        let _ = write!(
            s,
            "<item id=\"{i}\" m:kind=\"x\"><name>value {i}</name></item>"
        );
    }
    s.push_str("</root>");
    s
}

/// Reads whose cost mutation changes: a pre-order walk against the
/// arena scan, expanded-name lookup, and the document-order cache.
fn reads(c: &mut Criterion) {
    let doc = parse(&document(2000)).expect("well-formed");
    let root = doc.root_element().expect("a root element");
    let first = doc.children(root)[0];

    let mut group = c.benchmark_group("mutable_tree");
    group.bench_function("attribute_by_expanded_name", |b| {
        b.iter(|| black_box(&doc).attribute_ns(first, Some("urn:m"), "kind"));
    });
    group.bench_function("descendants_of_root_2000", |b| {
        b.iter(|| black_box(&doc).descendants_of(root).count());
    });
    // Computing a document-order position for every slot: what a
    // node-set sort pays once per batch of mutations. Each iteration
    // moves a node first, so the walk is measured and not a cached
    // read.
    group.bench_function("document_order_2000", |b| {
        let mut mutated = doc.clone();
        let moved = mutated.children(root)[1];
        b.iter(|| {
            mutated.reparent(moved, root).expect("live");
            black_box(mutated.document_order().len())
        });
    });
    group.finish();
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("arena_recycling");

    group.bench_function("recycle_1000_sequential_nodes", |b| {
        b.iter(|| {
            let mut doc = parse("<root/>").expect("parse");
            let root = doc.root_element().expect("root element");

            // Add 1000 nodes
            let mut ids = Vec::with_capacity(1000);
            for i in 0..1000 {
                let id =
                    doc.append_element(root, None, "node").expect("append");
                ids.push(id);
                black_box(i);
            }

            // Remove 1000 nodes (pushing to free_slots)
            for id in ids {
                doc.remove(id).expect("remove");
            }

            // Allocate 1000 more nodes (recycling from free_slots)
            for _ in 0..1000 {
                let _ = doc
                    .append_element(root, None, "recycled")
                    .expect("recycled");
            }

            black_box(doc)
        });
    });

    // Appending into one block. The first mutation API copied the
    // whole block on every append, which made this quadratic; a block
    // with spare capacity makes it linear.
    group.bench_function("append_10000_children", |b| {
        b.iter(|| {
            let mut doc = parse("<root/>").expect("parse");
            let root = doc.root_element().expect("root element");
            for _ in 0..10_000 {
                let _ = doc.append_element(root, None, "node").expect("append");
            }
            black_box(doc)
        });
    });

    // Attribute replacement overwrites the stored value in place.
    group.bench_function("replace_attribute_1000", |b| {
        let value = "v".repeat(256);
        b.iter(|| {
            let mut doc = parse("<root/>").expect("parse");
            let root = doc.root_element().expect("root element");
            for _ in 0..1_000 {
                doc.set_attribute(root, None, "k", &value).expect("set");
            }
            black_box(doc)
        });
    });

    // Removing an attribute node and then its element frees each slot
    // once; the appends afterwards reuse them.
    group.bench_function("remove_attribute_then_element_1000", |b| {
        b.iter(|| {
            let mut doc = parse("<root/>").expect("parse");
            let root = doc.root_element().expect("root element");
            for _ in 0..1_000 {
                let e = doc.append_element(root, None, "e").expect("append");
                doc.set_attribute(e, None, "k", "v").expect("set");
                let attr = doc.attribute_nodes(e)[0];
                doc.remove(attr).expect("remove attribute");
                doc.remove(e).expect("remove element");
            }
            black_box(doc)
        });
    });

    group.finish();
}

criterion_group!(benches, bench, reads);
criterion_main!(benches);
