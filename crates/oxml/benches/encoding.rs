// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Decoding, which runs before parsing and is charged to every
//! `parse_bytes` call.
//!
//! UTF-8 is the case worth protecting: it is not transcoded, so the
//! decoder should hand back a borrowed slice and cost approximately
//! nothing. The other encodings must allocate, and the gap between
//! them is the point of measuring all four together.

#![allow(missing_docs)] // criterion_group! generates an undocumented item

use core::fmt::Write as _;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn document(n: usize) -> String {
    let mut s = String::from("<?xml version=\"1.0\"?><root>");
    for i in 0..n {
        let _ = write!(s, "<item id=\"{i}\">value {i}</item>");
    }
    s.push_str("</root>");
    s
}

/// The same text as UTF-16 with a byte-order mark.
fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut out = if big_endian {
        alloc_bom(0xFE, 0xFF)
    } else {
        alloc_bom(0xFF, 0xFE)
    };
    for unit in text.encode_utf16() {
        let bytes = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        out.extend_from_slice(&bytes);
    }
    out
}

fn alloc_bom(a: u8, b: u8) -> Vec<u8> {
    vec![a, b]
}

/// The same text as ISO-8859-1, which is byte-per-character for the
/// ASCII range this document stays inside.
fn latin1(text: &str) -> Vec<u8> {
    let mut out =
        Vec::from(&b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>"[..]);
    let body = text.trim_start_matches("<?xml version=\"1.0\"?>");
    out.extend(body.chars().map(|c| c as u8));
    out
}

fn bench(c: &mut Criterion) {
    let text = document(1000);
    let utf8 = text.as_bytes().to_vec();
    let be = utf16(&text, true);
    let le = utf16(&text, false);
    let l1 = latin1(&text);

    let mut group = c.benchmark_group("encoding");
    // Should be far cheaper than the rest: nothing is copied.
    let _ = group.bench_function("decode_utf8_borrowed", |b| {
        b.iter(|| oxml::encoding::decode(black_box(&utf8)).unwrap());
    });
    let _ = group.bench_function("decode_utf16_be", |b| {
        b.iter(|| oxml::encoding::decode(black_box(&be)).unwrap());
    });
    let _ = group.bench_function("decode_utf16_le", |b| {
        b.iter(|| oxml::encoding::decode(black_box(&le)).unwrap());
    });
    let _ = group.bench_function("decode_latin1", |b| {
        b.iter(|| oxml::encoding::decode(black_box(&l1)).unwrap());
    });
    // Decode plus parse, which is what a caller actually pays.
    let _ = group.bench_function("parse_bytes_utf16_be", |b| {
        b.iter(|| oxml::parse_bytes(black_box(&be)).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
