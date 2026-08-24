// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Entity expansion, which is where a small input becomes a large
//! amount of work.
//!
//! The safety story is that expansion is bounded per *document*; the
//! performance story is what that bound costs on documents that are
//! not attacks. Both need measuring, because a limit that makes
//! ordinary entity use slow is a limit callers will raise.

#![allow(missing_docs)] // criterion_group! generates an undocumented item

use core::fmt::Write as _;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

/// A document using a declared entity `n` times.
fn with_entities(n: usize) -> String {
    let mut s =
        String::from("<!DOCTYPE d [<!ENTITY co \"Company Name Ltd\">]><d>");
    for i in 0..n {
        let _ = write!(s, "<p>{i} &co;</p>");
    }
    s.push_str("</d>");
    s
}

/// The same document with the text written out instead, so the cost of
/// expansion is separable from the cost of the characters it produces.
fn expanded(n: usize) -> String {
    let mut s = String::from("<d>");
    for i in 0..n {
        let _ = write!(s, "<p>{i} Company Name Ltd</p>");
    }
    s.push_str("</d>");
    s
}

/// Nested entities, each level ten copies of the last — the billion
/// laughs shape, kept well inside the default budget.
fn nested(levels: usize) -> String {
    let mut s = String::from("<!DOCTYPE d [<!ENTITY l0 \"haha\">");
    for i in 1..=levels {
        let prev = i - 1;
        let _ = write!(s, "<!ENTITY l{i} \"");
        for _ in 0..10 {
            let _ = write!(s, "&l{prev};");
        }
        s.push_str("\">");
    }
    let _ = write!(s, "]><d>&l{levels};</d>");
    s
}

fn bench(c: &mut Criterion) {
    let referenced = with_entities(1000);
    let literal = expanded(1000);
    let deep = nested(4); // 10^4 * 4 bytes, comfortably under the budget

    let mut group = c.benchmark_group("entities");
    let _ = group.bench_function("expand_1000_references", |b| {
        b.iter(|| oxml::parse(black_box(&referenced)).unwrap());
    });
    // The control: same output, no entities.
    let _ = group.bench_function("literal_equivalent", |b| {
        b.iter(|| oxml::parse(black_box(&literal)).unwrap());
    });
    let _ = group.bench_function("nested_four_levels", |b| {
        b.iter(|| oxml::parse(black_box(&deep)).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
