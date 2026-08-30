// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Building a document instead of parsing one.
//!
//! Run with:
//!
//! ```text
//! cargo run -p oxml --example build_a_document
//! ```
//!
//! Until 0.0.9 a `Document` could only come from a parser, so building
//! one meant writing XML into a string and parsing it back — slower,
//! and unable to express anything the serialiser would have escaped.

use oxml::tree::{Document, NodeError};

fn main() {
    let mut doc = Document::empty();
    let root = doc.root();

    let catalogue = doc
        .append_element(root, None, "catalogue")
        .expect("the root is live");

    for (title, year) in [("Dune", "1965"), ("Solaris", "1961")] {
        let book = doc.append_element(catalogue, None, "book").expect("live");
        let t = doc.append_element(book, None, "title").expect("live");
        let _ = doc.append_text(t, title).expect("live");
        let y = doc.append_element(book, None, "year").expect("live");
        let _ = doc.append_text(y, year).expect("live");
    }

    println!("{}", doc.to_xml());
    assert_eq!(doc.children(catalogue).len(), 2);

    // Text is stored as characters, not as the markup that spells
    // them. Escaping happens on the way out, so a caller writes what
    // it means.
    let note = doc.append_element(catalogue, None, "note").expect("live");
    let _ = doc.append_text(note, "Tom & Jerry <best>").expect("live");
    let xml = doc.to_xml();
    println!("\nwith escaping:\n{xml}");
    assert!(xml.contains("Tom &amp; Jerry &lt;best&gt;"), "{xml}");

    // What is built round-trips: parsing the output and serialising it
    // again produces the same text.
    let reparsed = oxml::parse(&xml).expect("what we built must parse");
    assert_eq!(
        reparsed.to_xml(),
        xml,
        "a built document must be a fixed point"
    );

    // Removing a node invalidates every identifier into that subtree.
    // A stale identifier is reported, not silently pointed at whatever
    // occupies the slot next — which is why `NodeId` carries the
    // generation of the slot it was minted for.
    doc.remove(note).expect("note is live");
    assert_eq!(doc.children(catalogue).len(), 2, "the note is unlinked");
    assert_eq!(
        doc.append_text(note, "too late"),
        Err(NodeError::Stale),
        "an identifier into a removed subtree must not resolve"
    );

    println!("\nafter removing the note:\n{}", doc.to_xml());
}
