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
//! Since 0.0.10 the tree can also be changed in place, and every
//! change is checked before it is applied: a name that is not a
//! name, a child under a text node, or a second root element is an
//! error at the call, not a surprise at the round trip.

use oxml::NodeId;
use oxml::tree::{Document, NodeError, NodeKind};

fn main() {
    let mut doc = Document::empty();
    let catalogue = build(&mut doc);
    let archive = remove_and_move(&mut doc, catalogue);
    gates(&mut doc, catalogue, archive);
    attributes_and_namespaces(&mut doc, catalogue, archive);
    order_and_compaction(&mut doc, catalogue);
    fluent();

    let xml = doc.to_xml();
    println!("\nafter moving a book into the archive:\n{xml}");
    assert_eq!(
        oxml::parse(&xml).expect("must parse").to_xml(),
        xml,
        "still a fixed point after mutation"
    );
}

/// The primitives: elements, text, comments and instructions.
fn build(doc: &mut Document) -> NodeId {
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

    // Comments and processing instructions are nodes like any other.
    // Both are written verbatim, so a comment may not contain `--`
    // and an instruction's data may not contain `?>`.
    let _ = doc
        .append_comment(catalogue, " prices exclude shipping ")
        .expect("live");
    let _ = doc
        .append_pi(root, "xml-stylesheet", "href=\"c.xsl\"")
        .expect("live");
    assert_eq!(
        doc.append_comment(catalogue, "a -- b"),
        Err(NodeError::InvalidText)
    );
    catalogue
}

/// Removing, moving, inserting at a position, and detaching.
fn remove_and_move(doc: &mut Document, catalogue: NodeId) -> NodeId {
    // Removing a node invalidates every identifier into that subtree.
    // A stale identifier is reported, not silently pointed at whatever
    // occupies the slot next — which is why `NodeId` carries the
    // generation of the slot it was minted for.
    let note = doc.children(catalogue)[2];
    doc.remove(note).expect("note is live");
    assert_eq!(doc.children(catalogue).len(), 3, "the note is unlinked");
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

    // `insert_at` is `reparent` with a position; past the end appends.
    let second = doc.children(catalogue)[0];
    doc.insert_at(archive, 0, second).expect("live");
    assert_eq!(doc.children(archive)[0], second, "inserted at the front");

    // `detach` unlinks without freeing: the subtree stays live, is
    // not written, and can be put back later.
    doc.detach(second).expect("live");
    assert_eq!(doc.parent(second), None);
    assert!(!doc.to_xml().contains("Solaris"), "{}", doc.to_xml());
    doc.reparent(second, archive)
        .expect("a detached subtree returns");
    archive
}

/// What is refused, and why each refusal is an error at the call.
fn gates(doc: &mut Document, catalogue: NodeId, archive: NodeId) {
    let root = doc.root();
    // A document may have only one root element, and the root itself
    // cannot be removed. Only the root and elements take children.
    assert_eq!(
        doc.append_element(root, None, "second-root"),
        Err(NodeError::RootElementExists)
    );
    assert_eq!(doc.remove(root), Err(NodeError::NotRemovable));
    let second = doc.children(archive)[0];
    let title = doc.children(second)[0];
    let title_text = doc.children(title)[0];
    assert_eq!(
        doc.append_element(title_text, None, "under-text"),
        Err(NodeError::NotAContainer)
    );

    // Names are validated: an element or attribute name must be an
    // `NCName`, and text must be something XML can write.
    assert_eq!(
        doc.append_element(catalogue, None, "not a name"),
        Err(NodeError::InvalidName)
    );
    assert_eq!(
        doc.append_text(catalogue, "\0"),
        Err(NodeError::InvalidText)
    );

    // Text nodes can be rewritten in place, and a single run of text
    // can be borrowed without gathering.
    doc.set_text(title_text, "Solaris (1961)")
        .expect("a text node");
    assert_eq!(doc.text_content(title), Some("Solaris (1961)"));
    assert_eq!(doc.text_content(title_text), Some("Solaris (1961)"));
    assert_eq!(
        doc.text_content(archive),
        None,
        "element children, so no single text to borrow"
    );
    assert_eq!(doc.set_text(title, "x"), Err(NodeError::NotText));
}

/// Attributes compare by expanded name; namespaces are declared on
/// the way out, or where the caller says.
fn attributes_and_namespaces(
    doc: &mut Document,
    catalogue: NodeId,
    archive: NodeId,
) {
    // Setting the same name twice replaces rather than appends -- XML
    // forbids duplicates on an element, and a document carrying two
    // would serialise to something that will not parse.
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

    // A namespaced name is given as its namespace *name* (the URI) and
    // local part. The serialiser reuses a prefix in scope or declares
    // one; `declare_namespace` chooses the prefix and where it lives.
    let _ = doc
        .declare_namespace(catalogue, Some("m"), "urn:example:meta")
        .expect("an element");
    let record = doc
        .append_element(archive, Some("urn:example:meta"), "record")
        .expect("live");
    doc.set_attribute(record, Some("urn:example:meta"), "id", "r-1")
        .expect("live");
    doc.set_attribute(record, None, "id", "plain")
        .expect("live");
    let xml = doc.to_xml();
    println!("\nwith namespaces:\n{xml}");
    assert!(xml.contains("<m:record "), "{xml}");

    // Lookup by expanded name, when the namespace matters. `attribute`
    // matches on the local part alone and finds the first.
    assert_eq!(
        doc.attribute_ns(record, Some("urn:example:meta"), "id"),
        Some("r-1")
    );
    assert_eq!(doc.attribute_ns(record, None, "id"), Some("plain"));
    assert_eq!(doc.attribute(record, "id"), Some("r-1"));
}

/// Document order after mutation, and reclaiming the arena.
fn order_and_compaction(doc: &mut Document, catalogue: NodeId) {
    let root = doc.root();
    // Arena order stops being document order once a node has moved;
    // `descendants_of` walks the tree in pre-order, and
    // `document_order` gives every slot a position to sort by,
    // computed once per batch of mutations.
    assert!(doc.is_order_dirty(), "a move invalidates the cached order");
    let order = doc.document_order().to_vec();
    assert!(!doc.is_order_dirty());
    let mut by_arena: Vec<_> =
        doc.nodes().filter(|n| doc.is_element(*n)).collect();
    let by_tree: Vec<_> = doc
        .descendants_of(root)
        .filter(|n| doc.is_element(*n))
        .collect();
    assert_ne!(
        by_arena, by_tree,
        "the arena is no longer in document order"
    );
    by_arena.sort_by_key(|n| order[n.index()]);
    assert_eq!(by_arena, by_tree, "sorted by position, it is");
    assert_eq!(doc.order_key(catalogue), Some(order[catalogue.index()]));

    // Every kind of node is reachable from `nodes`, attributes and
    // namespace declarations included; `len` counts the live ones.
    let declared = doc
        .nodes()
        .filter(|n| matches!(doc.kind(*n), Some(NodeKind::Namespace { .. })))
        .count();
    assert_eq!(declared, 1, "the one we declared");
    assert_eq!(doc.nodes().count(), doc.len());

    // After heavy mutation, `compact` rebuilds the arenas without the
    // garbage that moved blocks leave behind. Identifiers are stable.
    let before = doc.to_xml();
    doc.compact();
    assert_eq!(doc.to_xml(), before);
    assert_eq!(doc.order_key(catalogue), Some(order[catalogue.index()]));
}

/// The same tree, written as one expression.
///
/// The builder is a convenience over the primitives and does nothing
/// they cannot -- a test asserts the two produce identical output.
fn fluent() {
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
        .child_ns("urn:example:meta", "record", |r| {
            let _ = r
                .attr_ns("urn:example:meta", "id", "r-1")
                .attr("id", "plain");
        })
        .finish()
        .expect("the root is live");
    println!("\nbuilt fluently:\n{}", fluent.to_xml());
    assert_eq!(fluent.attribute(built, "version"), Some("1.0"));
    assert_eq!(fluent.text(froot), "Dune");

    // Errors are sticky rather than returned at every step, so a chain
    // reads as one expression and never half-applies silently.
    let failed = fluent.build(froot, "second-root").attr("k", "v").finish();
    assert_eq!(failed, Err(NodeError::RootElementExists));
}
