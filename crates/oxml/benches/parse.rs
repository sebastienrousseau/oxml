// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Parsing throughput.
//!
//! The documents are generated rather than fixtures so the shape is
//! explicit: a wide, shallow document stresses sibling handling, a
//! deep one stresses recursion, and an attribute-heavy one stresses
//! namespace resolution.

#![allow(missing_docs)] // criterion_group! generates an undocumented item

use core::fmt::Write as _;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn wide(n: usize) -> String {
    let mut s = String::from("<root>");
    for i in 0..n {
        let _ = write!(s, "<item id=\"{i}\">value {i}</item>");
    }
    s.push_str("</root>");
    s
}

fn deep(n: usize) -> String {
    let mut s = String::new();
    for _ in 0..n {
        s.push_str("<n>");
    }
    s.push_str("leaf");
    for _ in 0..n {
        s.push_str("</n>");
    }
    s
}

fn attr_heavy(n: usize) -> String {
    let mut s = String::from("<root xmlns:x=\"urn:example\">");
    for i in 0..n {
        let _ = write!(s, "<x:e a=\"{i}\" b=\"{i}\" c=\"{i}\" x:d=\"{i}\"/>");
    }
    s.push_str("</root>");
    s
}

fn bench(c: &mut Criterion) {
    let wide_doc = wide(1000);
    let deep_doc = deep(500);
    let attr_doc = attr_heavy(1000);

    let mut group = c.benchmark_group("parse");
    let _ = group.bench_function("wide_1000", |b| {
        b.iter(|| oxml::parse(black_box(&wide_doc)).unwrap());
    });
    let _ = group.bench_function("deep_500", |b| {
        b.iter(|| oxml::parse(black_box(&deep_doc)).unwrap());
    });
    let _ = group.bench_function("attributes_1000", |b| {
        b.iter(|| oxml::parse(black_box(&attr_doc)).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
