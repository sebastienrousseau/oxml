// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Bytes per second, on documents whose size is known.
//!
//! The roadmap's target is stated in throughput and nothing measured
//! it: the other benchmarks report time per document, which cannot be
//! compared against a figure in MB/s without knowing each document's
//! size. Criterion is told the byte count here, so it reports
//! throughput directly.
//!
//! **A number from this file is not publishable on its own.** The rule
//! in `doc/BENCHMARKS.md` is that a figure carries its machine,
//! toolchain, load average and confidence interval, because the same
//! binary measured 14.7 and 123.1 MB/s on one machine on one day. Use
//! `scripts/record-throughput.sh`, which refuses to record when the
//! machine is busy.

#![allow(missing_docs)] // criterion_group! generates an undocumented item

use core::fmt::Write as _;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

/// A catalogue: the shape most XML in the wild actually has.
fn catalogue(items: usize) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <catalogue xmlns=\"urn:example\" xmlns:m=\"urn:meta\">",
    );
    for i in 0..items {
        let _ = write!(
            s,
            "<item id=\"i{i}\" m:sku=\"S{i:06}\">\
             <name>Product number {i}</name>\
             <price currency=\"GBP\">{}.{:02}</price>\
             <description>A description of product {i}, long enough to \
             be representative of real character data.</description>\
             </item>",
            i % 500,
            i % 100
        );
    }
    s.push_str("</catalogue>");
    s
}

/// Text-dominated rather than markup-dominated.
fn prose(paragraphs: usize) -> String {
    let mut s = String::from("<book>");
    for i in 0..paragraphs {
        let _ = write!(
            s,
            "<p>Paragraph {i}. {}</p>",
            "The quick brown fox jumps over the lazy dog. ".repeat(8)
        );
    }
    s.push_str("</book>");
    s
}

/// Attribute-dominated, which stresses name resolution.
fn attributes(rows: usize) -> String {
    let mut s = String::from("<table xmlns:c=\"urn:cols\">");
    for i in 0..rows {
        let _ = write!(
            s,
            "<row a=\"{i}\" b=\"{i}\" c:c=\"{i}\" c:d=\"{i}\" e=\"{i}\"/>"
        );
    }
    s.push_str("</table>");
    s
}

fn bench(c: &mut Criterion) {
    let cases = [
        ("catalogue", catalogue(5_000)),
        ("prose", prose(5_000)),
        ("attributes", attributes(20_000)),
    ];

    let mut group = c.benchmark_group("throughput");
    for (name, source) in &cases {
        // Telling criterion the size is what turns a duration into a
        // rate; without it the target cannot be checked at all.
        let _ = group.throughput(Throughput::Bytes(source.len() as u64));
        let _ = group.bench_function(*name, |b| {
            b.iter(|| oxml::parse(black_box(source)).unwrap());
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
