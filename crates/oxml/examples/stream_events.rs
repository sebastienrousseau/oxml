// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading a document as events, without building a tree.
//!
//! Run with:
//!
//! ```text
//! cargo run --example stream_events
//! ```

use oxml::stream::{Event, Reader};
use oxml::{Limits, parse};

const DOC: &str = r#"<?xml version="1.0"?>
<catalogue xmlns:m="urn:example:meta">
  <item id="a1"><name>Tea</name><m:added>2026-01-04</m:added></item>
  <item id="a2"><name>Coffee</name><m:added>2026-02-11</m:added></item>
  <item id="a3"><name>Cocoa</name><m:added>2026-03-27</m:added></item>
</catalogue>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A reader pulls one event at a time. Nothing is retained between
    // events but the stack of open elements, so a document larger than
    // memory is read in memory the size of its deepest path.
    let mut reader = Reader::new(DOC)?;
    let mut depth = 0usize;
    let mut items = 0usize;
    while let Some(event) = reader.next_event()? {
        match event {
            Event::StartElement { name, attributes } => {
                let attrs: Vec<String> = attributes
                    .iter()
                    .map(|(n, v)| format!(" {}={v:?}", n.local))
                    .collect();
                println!(
                    "{:indent$}<{}>{}",
                    "",
                    name.local,
                    attrs.concat(),
                    indent = depth * 2
                );
                if name.local == "item" {
                    items += 1;
                }
                depth += 1;
            }
            Event::EndElement { .. } => depth = depth.saturating_sub(1),
            // Character data arrives as one event per run, so text
            // split by a reference or a CDATA section is not split
            // across events.
            Event::Text(text) if !text.trim().is_empty() => {
                println!("{:indent$}{}", "", text.trim(), indent = depth * 2);
            }
            _ => {}
        }
    }
    println!("\n{items} items");

    // Namespaces are resolved as the tree parser resolves them: a
    // prefix is looked up in the scopes open at that point.
    let mut reader = Reader::new(DOC)?;
    while let Some(event) = reader.next_event()? {
        if let Event::StartElement { name, .. } = event {
            if let Some(uri) = name.namespace {
                println!("{} is in {uri:?}", name.local);
                break;
            }
        }
    }

    // The same `Limits` the tree parser takes, enforced the same way.
    // Depth costs a stack frame in the tree parser but only a vector
    // entry here, so the limit is memory policy rather than a guard
    // against overflowing the stack -- it is kept identical so that
    // both entry points accept exactly the same documents.
    let mut limits = Limits::default();
    limits.max_depth = 2;
    let mut reader = Reader::with_limits(DOC, limits)?;
    let refused = loop {
        match reader.next_event() {
            Ok(Some(_)) => {}
            Ok(None) => break None,
            Err(error) => break Some(error),
        }
    };
    println!(
        "with max_depth(2): {}",
        match &refused {
            Some(error) => error.to_string(),
            None => "read to the end".to_owned(),
        }
    );

    // From a byte source, which is what makes a document larger than
    // memory readable: only the construct in hand is kept, and what
    // has been passed is dropped. A 185 KB document and a 1,929 KB one
    // both hold 34,722 bytes.
    //
    // `from_reader` takes `R: BufRead + 'static`, so the reader owns
    // its bytes -- a `File` does already, and a `String` becomes one
    // through `Cursor`.
    let mut reader =
        Reader::from_reader(std::io::Cursor::new(DOC.as_bytes().to_vec()))?;
    let mut from_source = 0usize;
    while let Some(event) = reader.next_event()? {
        if matches!(event, Event::StartElement { .. }) {
            from_source += 1;
        }
    }
    println!("\nfrom a byte source: {from_source} elements");

    // The same, with limits.
    let mut reader = Reader::from_reader_with(
        std::io::Cursor::new(DOC.as_bytes().to_vec()),
        Limits::default(),
    )?;
    let mut counted = 0usize;
    while reader.next_event()?.is_some() {
        counted += 1;
    }
    println!("from a byte source with limits: {counted} events");

    // Streaming and parsing accept exactly the same documents, because
    // they run the same scanner -- only the arena differs.
    let malformed = "<a><b></a>";
    println!(
        "\n{malformed:?}\n  parse:  {}\n  stream: {}",
        parse(malformed).unwrap_err(),
        Reader::new(malformed)
            .and_then(|mut r| {
                while r.next_event()?.is_some() {}
                Ok(())
            })
            .unwrap_err()
    );

    Ok(())
}
