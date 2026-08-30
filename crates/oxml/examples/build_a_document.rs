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

    // Moving a subtree. A node cannot be moved inside its own subtree:
    // the result would be a tree containing a loop, and every walker
    // here follows children until they run out.
    let archive = doc
        .append_element(catalogue, None, "archive")
        .expect("live");
    let first = doc.children(catalogue)[0];
    doc.reparent(first, archive).expect("no cycle");
    assert_eq!(doc.children(archive).len(), 1, "the book moved");

    assert_eq!(
        doc.reparent(catalogue, archive),
        Err(NodeError::WouldCycle),
        "moving a node into its own descendant must be refused"
    );

    // A document may have only one root element.
    assert_eq!(
        doc.append_element(root, None, "second-root"),
        Err(NodeError::RootElementExists)
    );

    // Attributes. Setting the same name twice replaces rather than
    // appends -- XML forbids duplicates on an element, and a document
    // carrying two would serialise to something that will not parse.
    doc.set_attribute(archive, None, "count", "0")
        .expect("live");
    doc.set_attribute(archive, None, "count", "1")
        .expect("live");
    assert_eq!(doc.attribute(archive, "count"), Some("1"));
    assert_eq!(doc.attribute_nodes(archive).len(), 1);

    assert_eq!(doc.remove_attribute(archive, None, "count"), Ok(true));
    assert_eq!(
        doc.remove_attribute(archive, None, "count"),
        Ok(false),
        "removing it again is not an error, just nothing to do"
    );

    // The same tree, written as one expression. The builder is a
    // convenience over the primitives above and does nothing they
    // cannot -- a test asserts the two produce identical output.
    let mut fluent = Document::empty();
    let froot = fluent.root();
    let built = fluent
        .build(froot, "catalogue")
        .attr("version", "1.0")
        .child("book", |b| {
            let _ = b.attr("year", "1965").child("title", |t| {
                let _ = t.text("Dune");
            });
        })
        .finish()
        .expect("the root is live");
    println!("\nbuilt fluently:\n{}", fluent.to_xml());
    assert_eq!(fluent.attribute(built, "version"), Some("1.0"));
    assert_eq!(fluent.text(froot), "Dune");

    // A namespaced attribute. An unprefixed attribute is in *no*
    // namespace rather than the element's, so the two are different
    // names and both can sit on one element.
    let ns_doc = fluent
        .build(built, "record")
        .attr_ns("urn:example:meta", "id", "r-1")
        .attr("id", "plain")
        .finish()
        .expect("live");
    // Both are stored: an unprefixed attribute is in *no* namespace
    // rather than the element's, so `urn:example:meta`-`id` and plain
    // `id` are different names.
    assert_eq!(
        fluent.attribute_nodes(ns_doc).len(),
        2,
        "two distinct names"
    );

    // But `attribute` looks up by *local* name and ignores
    // namespaces, which is what a caller almost always means -- so
    // with two attributes sharing a local name it answers with the
    // first, here the namespaced one.
    assert_eq!(fluent.attribute(ns_doc, "id"), Some("r-1"));

    // Errors are sticky rather than returned at every step, so a chain
    // reads as one expression and never half-applies silently.
    let failed = fluent.build(froot, "second-root").attr("k", "v").finish();
    assert_eq!(failed, Err(NodeError::RootElementExists));

    let xml = doc.to_xml();
    println!("\nafter moving a book into the archive:\n{xml}");
    assert_eq!(
        oxml::parse(&xml).expect("must parse").to_xml(),
        xml,
        "still a fixed point after mutation"
    );
}
