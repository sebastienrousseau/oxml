// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `XPath` evaluation cost.
//!
//! Compilation and evaluation are timed separately: compiling is
//! per-expression and evaluation is per-document, so a caller reusing
//! a compiled query pays only the second. Reporting one combined
//! number would hide that.

#![allow(missing_docs)] // criterion_group! generates an undocumented item

use core::fmt::Write as _;
use criterion::{Criterion, criterion_group, criterion_main};
use oxml::{XPath, parse};
use std::hint::black_box;

fn document(n: usize) -> String {
    let mut s = String::from("<library>");
    for i in 0..n {
        let lang = if i % 2 == 0 { "en" } else { "fr" };
        let _ = write!(
            s,
            "<book lang=\"{lang}\" year=\"{}\"><title>T{i}</title></book>",
            1900 + i % 120
        );
    }
    s.push_str("</library>");
    s
}

/// `n` empty elements under one root.
fn flat_document(n: usize) -> String {
    let mut s = String::with_capacity(n * 4 + 8);
    s.push_str("<r>");
    for _ in 0..n {
        s.push_str("<e/>");
    }
    s.push_str("</r>");
    s
}

fn bench(c: &mut Criterion) {
    let src = document(2000);
    let doc = parse(&src).expect("valid fixture");

    let mut group = c.benchmark_group("xpath");
    let _ = group.bench_function("compile", |b| {
        b.iter(|| {
            XPath::compile(black_box("//book[@lang='en']/title")).unwrap()
        });
    });

    let simple = XPath::compile("//title").unwrap();
    let _ = group.bench_function("eval_descendant", |b| {
        b.iter(|| black_box(simple.evaluate(&doc)));
    });

    let predicate = XPath::compile("//book[@lang='en']/title").unwrap();
    let _ = group.bench_function("eval_predicate", |b| {
        b.iter(|| black_box(predicate.evaluate(&doc)));
    });

    let numeric = XPath::compile("//book[@year>1950]").unwrap();
    let _ = group.bench_function("eval_numeric_predicate", |b| {
        b.iter(|| black_box(numeric.evaluate(&doc)));
    });

    // `following::` and `preceding::` from every element: 6,001
    // context nodes on a document of 6,001 elements. A scan of the
    // arena per context node is quadratic in the document; a walk
    // of the tree is linear in what it yields.
    let flat = parse(&flat_document(6_000)).expect("valid fixture");
    let following = XPath::compile("count(//*/following::*)").unwrap();
    let _ = group.bench_function("eval_following_from_every_element", |b| {
        b.iter(|| black_box(following.evaluate(&flat)));
    });
    let preceding = XPath::compile("count(//*/preceding::*)").unwrap();
    let _ = group.bench_function("eval_preceding_from_every_element", |b| {
        b.iter(|| black_box(preceding.evaluate(&flat)));
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
