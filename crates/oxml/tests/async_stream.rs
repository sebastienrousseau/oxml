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
        if let Event::StartElement { name, attributes } = event {
            if name.local == "item" {
                if let Some((_, val)) =
                    attributes.iter().find(|(attr, _)| attr.local == "id")
                {
                    item_ids.push(val.clone());
                }
            }
        }
    }

    assert_eq!(item_ids, vec!["1", "2"]);
}
