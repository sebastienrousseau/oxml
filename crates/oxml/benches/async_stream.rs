// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Benchmarking `AsyncReader` event streaming performance.
//!
//! Measures throughput of reading and decoding XML event streams asynchronously
//! over Tokio runtime.

#![allow(missing_docs, unused_results)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxml::stream::AsyncReader;
use std::fmt::Write as _;
use std::hint::black_box;
use std::io::Cursor;

fn generate_xml(n: usize) -> Vec<u8> {
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><catalog>");
    for i in 0..n {
        let _ = write!(
            xml,
            r#"<item id="{i}" status="active"><name>Item {i}</name><value>{i}</value></item>"#
        );
    }
    xml.push_str("</catalog>");
    xml.into_bytes()
}

fn bench(c: &mut Criterion) {
    let payload = generate_xml(500);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let mut group = c.benchmark_group("async_stream");

    group.bench_function("async_stream_500_items", |b| {
        b.iter(|| {
            rt.block_on(async {
                let cursor = Cursor::new(&payload);
                let mut reader =
                    AsyncReader::from_reader(cursor).await.expect("reader");
                let mut count = 0;
                while let Some(evt) = reader.next_event().await.expect("event")
                {
                    black_box(evt);
                    count += 1;
                }
                black_box(count);
            });
        });
    });

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
