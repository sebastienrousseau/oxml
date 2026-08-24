// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading a document that is already parsed.
//!
//! The tree API is a capability in its own right — a caller who never
//! writes an `XPath` expression still pays these costs — and it is
//! half most sensitive to the arena layout. Traversal should be a walk
//! over contiguous indices; `text` must gather a subtree's character
//! data; attribute lookup is by expanded name, not by position.

#![allow(missing_docs)] // criterion_group! generates an undocumented item

use core::fmt::Write as _;
use criterion::{Criterion, criterion_group, criterion_main};
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

fn bench(c: &mut Criterion) {
    let doc = oxml::parse(&document(2000)).expect("well-formed");
    let root = doc.root_element().expect("a root element");
    let first = doc.children(root)[0];

    let mut group = c.benchmark_group("tree");
    // A full walk: the arena order makes this a linear scan.
    let _ = group.bench_function("descendants_2000", |b| {
        b.iter(|| black_box(&doc).descendants().count());
    });
    // Recursive gather of every text node under the root.
    let _ = group.bench_function("text_of_root", |b| {
        b.iter(|| black_box(&doc).text(root));
    });
    // Walking children through the shared `child_ids` arena.
    let _ = group.bench_function("children_of_each", |b| {
        b.iter(|| {
            let d = black_box(&doc);
            d.children(root)
                .iter()
                .map(|&c| d.children(c).len())
                .sum::<usize>()
        });
    });
    // Lookup by local name, which scans the element's attributes.
    let _ = group.bench_function("attribute_by_name", |b| {
        b.iter(|| black_box(&doc).attribute(first, "id"));
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
