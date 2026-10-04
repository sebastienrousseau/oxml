// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading an XML document asynchronously using `oxml::stream::AsyncReader`.
//!
//! Demonstrates non-blocking event-based streaming over a byte source using
//! Tokio's `AsyncBufRead`. Events are processed one at a time with bounded
//! memory consumption.

use oxml::stream::{AsyncReader, Event};
use oxml::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let xml_payload = r#"<?xml version="1.0" encoding="UTF-8"?>
    <inventory store="London Flagship">
        <item sku="OX-001" category="books">
            <title>The Rust Programming Language</title>
            <price currency="GBP">35.00</price>
            <stock>42</stock>
        </item>
        <item sku="OX-002" category="electronics">
            <title>Mechanical Keyboard</title>
            <price currency="GBP">120.00</price>
            <stock>15</stock>
        </item>
    </inventory>"#;

    // Wrap in-memory bytes or any network/socket stream
    let cursor = std::io::Cursor::new(xml_payload.as_bytes());
    let mut reader = AsyncReader::from_reader(cursor).await?;

    let mut current_item_sku = None;
    let mut item_count = 0;

    println!("Starting asynchronous XML streaming event loop...\n");

    while let Some(event) = reader.next_event().await? {
        match event {
            Event::StartElement { name, attributes } => {
                if name.local == "item" {
                    item_count += 1;
                    if let Some((_, sku)) = attributes.iter().find(|(attr, _)| attr.local == "sku") {
                        current_item_sku = Some(sku.clone());
                    }
                } else if name.local == "title" {
                    if let Some(sku) = &current_item_sku {
                        print!("Item [{sku}]: ");
                    }
                }
            }
            Event::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    println!("{trimmed}");
                }
            }
            Event::EndElement { name } => {
                if name.local == "item" {
                    current_item_sku = None;
                }
            }
            Event::Comment(comment) => {
                println!("Comment found: {comment}");
            }
            _ => {}
        }
    }

    println!("\nSuccessfully streamed {item_count} items with AsyncReader.");
    Ok(())
}
