// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Asynchronous stream reader integration tests.

#![cfg(feature = "async")]

use oxml::Limits;
use oxml::stream::{AsyncReader, Event, Reader};
use std::io::Cursor;

#[tokio::test]
async fn async_reader_matches_sync_reader_events() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <!-- A catalog of books -->
    <catalog name="main">
        <book id="bk101">
            <author>Gambardella, Matthew</author>
            <title>XML Developer's Guide</title>
            <price currency="USD">44.95</price>
            <empty/>
        </book>
    </catalog>"#;

    // Read synchronously
    let mut sync_reader =
        Reader::from_reader(Cursor::new(xml.as_bytes())).unwrap();
    let mut sync_events = Vec::new();
    while let Some(evt) = sync_reader.next_event().unwrap() {
        sync_events.push(evt);
    }

    // Read asynchronously
    let mut async_reader =
        AsyncReader::from_reader(Cursor::new(xml.as_bytes()))
            .await
            .unwrap();
    let mut async_events = Vec::new();
    while let Some(evt) = async_reader.next_event().await.unwrap() {
        async_events.push(evt);
    }

    assert_eq!(sync_events, async_events);
    assert!(!async_events.is_empty());
}

#[tokio::test]
async fn async_reader_respects_limits() {
    let xml = "<root><a><b><c><d/></c></b></a></root>";
    let mut limits = Limits::default();
    limits.max_depth = 3;

    let mut async_reader =
        AsyncReader::from_reader_with(Cursor::new(xml.as_bytes()), limits)
            .await
            .unwrap();

    let mut err = None;
    loop {
        match async_reader.next_event().await {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }

    assert!(err.is_some(), "expected depth limit error");
}

#[tokio::test]
async fn async_reader_handles_incremental_chunks() {
    let xml =
        "<items><item id='1'>First</item><item id='2'>Second</item></items>";
    let mut async_reader =
        AsyncReader::from_reader(Cursor::new(xml.as_bytes()))
            .await
            .unwrap();

    let mut item_ids = Vec::new();
    while let Some(event) = async_reader.next_event().await.unwrap() {
        if let Event::StartElement {
            name, attributes, ..
        } = event
            && name.local == "item"
            && let Some((_, val)) =
                attributes.iter().find(|(attr, _)| attr.local == "id")
        {
            item_ids.push(val.clone());
        }
    }

    assert_eq!(item_ids, vec!["1", "2"]);
}

/// An asynchronous source that never ends: a prefix, then `x` forever.
struct Endless {
    prefix: Vec<u8>,
    at: usize,
    handed_out: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl tokio::io::AsyncRead for Endless {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let n = buf.remaining().min(8192);
        let mut chunk = vec![b'x'; n];
        for slot in &mut chunk {
            if let Some(&b) = self.prefix.get(self.at) {
                *slot = b;
            }
            self.at += 1;
        }
        buf.put_slice(&chunk);
        let _ = self
            .handed_out
            .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        std::task::Poll::Ready(Ok(()))
    }
}

/// The asynchronous reader applies the same construct bound as the
/// synchronous one, and gives up having read about as much.
#[tokio::test]
async fn async_reader_refuses_an_endless_construct_within_the_limit() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let limits = Limits::strict();
    let cap = limits.max_construct_bytes.expect("strict() bounds it");
    for prefix in ["<a>", "<!--"] {
        let handed_out = Arc::new(AtomicUsize::new(0));
        let source = tokio::io::BufReader::new(Endless {
            prefix: prefix.as_bytes().to_vec(),
            at: 0,
            handed_out: Arc::clone(&handed_out),
        });
        let mut reader = AsyncReader::from_reader_with(source, limits)
            .await
            .expect("prolog is fine");
        let error = loop {
            match reader.next_event().await {
                Ok(Some(_)) => {}
                Ok(None) => panic!("{prefix:?} never ends"),
                Err(e) => break e,
            }
        };
        assert_eq!(
            error.kind,
            oxml::ErrorKind::ConstructTooLarge,
            "{prefix:?}"
        );
        let swallowed = handed_out.load(Ordering::Relaxed);
        assert!(
            swallowed <= cap + 64 * 1024,
            "{prefix:?}: read {swallowed} bytes against a cap of {cap}"
        );
    }
}

/// A source that yields one byte per poll.
struct TrickleAsync {
    data: Vec<u8>,
    at: usize,
}

impl tokio::io::AsyncRead for TrickleAsync {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if let Some(&byte) = self.data.get(self.at) {
            buf.put_slice(&[byte]);
            self.at += 1;
        }
        std::task::Poll::Ready(Ok(()))
    }
}

/// One byte per poll puts a boundary between every pair of bytes,
/// inside multi-byte characters and across `\r\n` pairs.
#[tokio::test]
async fn async_reader_reads_a_byte_at_a_time() {
    let xml = "<?xml version=\"1.1\"?>\r\n<!DOCTYPE a [<!ENTITY e \"\u{e9}\">]>\
               <a x='\u{1F600}'>one <![CDATA[two]]> &e;\r\u{85}three</a>";
    let mut sync_reader = Reader::new(xml).unwrap();
    let mut expected = Vec::new();
    while let Some(event) = sync_reader.next_event().unwrap() {
        expected.push(event);
    }
    let source = tokio::io::BufReader::with_capacity(
        1,
        TrickleAsync {
            data: xml.as_bytes().to_vec(),
            at: 0,
        },
    );
    let mut reader = AsyncReader::from_reader(source).await.unwrap();
    let mut got = Vec::new();
    while let Some(event) = reader.next_event().await.unwrap() {
        got.push(event);
    }
    assert_eq!(got, expected);
    assert_eq!(
        reader.offset(),
        sync_reader.offset(),
        "the end of the decoded document"
    );
}

/// The future `next_event` returns is `Send`, so a reader can be
/// driven from a spawned task -- including with `tracing` on, where a
/// span held across an await point would make it `!Send`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_reader_can_be_driven_from_a_spawned_task() {
    fn assert_send<T: Send>(_: &T) {}
    let mut reader =
        AsyncReader::from_reader(Cursor::new(b"<a>x</a>".to_vec()))
            .await
            .unwrap();
    assert_send(&reader.next_event());
    assert_send(&reader);

    let names = tokio::spawn(async move {
        let mut names = Vec::new();
        while let Some(event) = reader.next_event().await.unwrap() {
            if let Event::StartElement { name, .. } = event {
                names.push(name.local);
            }
        }
        names
    })
    .await
    .unwrap();
    assert_eq!(names, ["a"]);
}

/// An external source reaches the DTD parser through the asynchronous
/// reader too.
#[tokio::test]
async fn async_reader_takes_an_external_source() {
    struct Dtd;
    impl oxml::external::ExternalSource for Dtd {
        fn fetch(&self, system: &str, _public: Option<&str>) -> Option<&str> {
            (system == "x.dtd").then_some("<!ENTITY e \"outside\">")
        }
    }
    let source = Dtd;
    let doc = b"<!DOCTYPE a SYSTEM \"x.dtd\"><a>&e;</a>";
    let mut reader = AsyncReader::from_reader(Cursor::new(doc.to_vec()))
        .await
        .unwrap()
        .with_external(&source);
    let mut text = String::new();
    while let Some(event) = reader.next_event().await.unwrap() {
        if let Event::Text(t) = event {
            text.push_str(&t);
        }
    }
    assert_eq!(text, "outside");
}
