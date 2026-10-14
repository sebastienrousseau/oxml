// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! A mutable tree you can trust.
//!
//! Each test here is a defect that the first mutation API had, written
//! as the behaviour the fixed one guarantees. The unit tests beside
//! the code check that each operation does what it says; these check
//! that no operation can leave the document in a state another one
//! misreads -- a slot freed twice, a child under a text node, a name
//! the serialiser cannot spell.

use oxml::tree::{Document, NodeError, NodeKind};
use oxml::{NodeId, parse};

/// The element with the given local name, wherever it is.
fn find(d: &Document, local: &str) -> NodeId {
    d.nodes()
        .find(|n| d.element_name(*n).is_some_and(|e| e.local == local))
        .unwrap_or_else(|| panic!("no element named {local}"))
}

/// A document with one root element, and that element.
fn built() -> (Document, NodeId) {
    let mut doc = Document::empty();
    let root = doc.root();
    let top = doc.append_element(root, None, "top").expect("live");
    (doc, top)
}

/// Serialise, reparse, and check the text is a fixed point.
fn round_trip(doc: &Document) -> Document {
    let once = doc.to_xml();
    let again = parse(&once)
        .unwrap_or_else(|e| panic!("output does not parse: {e}\n{once}"));
    assert_eq!(again.to_xml(), once, "not a fixed point");
    again
}

// --- slots are freed exactly once -------------------------------------

#[test]
fn removing_an_attribute_node_then_its_element_frees_each_slot_once() {
    // `remove(attr)` freed the slot but left the element's attribute
    // range pointing at it; `remove(element)` then walked the range
    // and freed it again. The generation went odd then even, the slot
    // was on the free list twice, and the next two allocations shared
    // one slot: an identifier that did not resolve and a node that
    // vanished from the output.
    let (mut doc, top) = built();
    let holder = doc.append_element(top, None, "holder").expect("live");
    doc.set_attribute(holder, None, "k", "v").expect("live");
    let attr = doc.attribute_nodes(holder)[0];

    doc.remove(attr).expect("an attribute node can be removed");
    assert!(
        doc.attribute_nodes(holder).is_empty(),
        "unlinked from owner"
    );
    doc.remove(holder).expect("live");

    let a = doc.append_element(top, None, "a").expect("live");
    let b = doc.append_element(top, None, "b").expect("live");
    assert_ne!(a, b, "two allocations handed out one slot");
    assert_eq!(doc.element_name(a).map(|n| n.local.as_str()), Some("a"));
    assert_eq!(doc.element_name(b).map(|n| n.local.as_str()), Some("b"));
    assert_eq!(doc.to_xml(), "<top><a/><b/></top>");
}

#[test]
fn removing_an_element_frees_its_namespace_nodes() {
    // The doomed walk collected children and attributes, never
    // namespace nodes, so every `xmlns` on a removed element stayed
    // live with a parent that no longer resolved.
    let mut doc =
        parse("<r><a xmlns:p=\"urn:p\"><p:b/></a><c/></r>").expect("valid");
    let a = find(&doc, "a");
    doc.remove(a).expect("live");

    for id in doc.nodes() {
        if let Some(NodeKind::Namespace { .. }) = doc.kind(id) {
            let parent = doc.parent(id).expect("a namespace node has an owner");
            assert!(
                doc.kind(parent).is_some(),
                "namespace node {id:?} survives its element"
            );
        }
    }
    assert_eq!(doc.len(), doc.nodes().count(), "len counts live nodes");
}

// --- containers and the root ----------------------------------------

#[test]
fn only_the_root_and_elements_take_children() {
    // A child appended under a text node was stored and silently
    // dropped on serialisation.
    let (mut doc, top) = built();
    let text = doc.append_text(top, "t").expect("live");
    doc.set_attribute(top, None, "k", "v").expect("live");
    let attr = doc.attribute_nodes(top)[0];
    let comment = doc.append_comment(top, "c").expect("live");

    for bad in [text, attr, comment] {
        assert_eq!(
            doc.append_element(bad, None, "e"),
            Err(NodeError::NotAContainer)
        );
        assert_eq!(doc.append_text(bad, "x"), Err(NodeError::NotAContainer));
        assert_eq!(doc.build(bad, "e").finish(), Err(NodeError::NotAContainer));
        assert!(doc.children(bad).is_empty(), "nothing was attached");
    }
    let movable = doc.append_element(top, None, "m").expect("live");
    assert_eq!(doc.reparent(movable, text), Err(NodeError::NotAContainer));
    assert_eq!(
        doc.parent(movable),
        Some(top),
        "a refused move changes nothing"
    );
}

#[test]
fn the_document_root_keeps_its_one_element() {
    // `reparent(x, root)` bypassed the single-root-element rule that
    // `append_element` enforces, and `remove(root)` bricked the
    // document.
    let (mut doc, top) = built();
    let inner = doc.append_element(top, None, "inner").expect("live");
    let root = doc.root();

    assert_eq!(doc.reparent(inner, root), Err(NodeError::RootElementExists));
    assert_eq!(doc.remove(root), Err(NodeError::NotRemovable));
    assert_eq!(doc.detach(root), Err(NodeError::NotRemovable));
    assert_eq!(doc.reparent(root, top), Err(NodeError::NotRemovable));

    // Moving the root element to the root is the one legal case: it
    // is already the root element.
    doc.reparent(top, root).expect("already the root element");
    assert_eq!(doc.root_element(), Some(top));

    // And after removing the root element a new one is accepted.
    doc.remove(top).expect("live");
    assert!(doc.root_element().is_none());
    let fresh = doc.append_element(root, None, "fresh").expect("live");
    assert_eq!(doc.root_element(), Some(fresh));
    assert_eq!(doc.to_xml(), "<fresh/>");
}

#[test]
fn attribute_and_namespace_nodes_are_not_children() {
    let (mut doc, top) = built();
    doc.set_attribute(top, None, "k", "v").expect("live");
    let attr = doc.attribute_nodes(top)[0];
    let ns = doc
        .declare_namespace(top, Some("p"), "urn:p")
        .expect("live element");
    let holder = doc.append_element(top, None, "holder").expect("live");

    for not_a_child in [attr, ns] {
        assert_eq!(
            doc.reparent(not_a_child, holder),
            Err(NodeError::NotAChild)
        );
        assert_eq!(
            doc.insert_at(holder, 0, not_a_child),
            Err(NodeError::NotAChild)
        );
        assert_eq!(doc.detach(not_a_child), Err(NodeError::NotAChild));
    }
    assert!(doc.children(holder).is_empty());
    assert_eq!(
        doc.to_xml(),
        "<top xmlns:p=\"urn:p\" k=\"v\"><holder/></top>"
    );
}

// --- validation ---------------------------------------------------------

#[test]
fn names_must_be_ncnames() {
    let (mut doc, top) = built();
    for bad in ["", "1x", "bad name", "a:b", "<", "-a"] {
        assert_eq!(
            doc.append_element(top, None, bad),
            Err(NodeError::InvalidName),
            "element {bad:?}"
        );
        assert_eq!(
            doc.set_attribute(top, None, bad, "v"),
            Err(NodeError::InvalidName),
            "attribute {bad:?}"
        );
        // `Some("")` is the default namespace, the same as `None`,
        // so the empty string is a valid prefix and not tested here.
        if !bad.is_empty() {
            assert_eq!(
                doc.declare_namespace(top, Some(bad), "urn:x"),
                Err(NodeError::InvalidName),
                "prefix {bad:?}"
            );
        }
    }
    for good in ["a", "_a", "a-b.c", "\u{e9}l\u{e9}ment", "a1"] {
        assert!(doc.append_element(top, None, good).is_ok(), "{good:?}");
    }
    // The reserved prefixes cannot be declared.
    for reserved in ["xml", "xmlns"] {
        assert_eq!(
            doc.declare_namespace(top, Some(reserved), "urn:x"),
            Err(NodeError::InvalidName)
        );
    }
    assert!(doc.attribute_nodes(top).is_empty(), "nothing was stored");
    let _ = round_trip(&doc);
}

#[test]
fn text_must_be_xml_characters() {
    // NUL, U+FFFE and U+FFFF cannot be written in any XML version,
    // literally or as a reference. Everything else can: a control
    // character goes out as a reference under a 1.1 declaration.
    let (mut doc, top) = built();
    for bad in ["\0", "a\u{fffe}", "\u{ffff}"] {
        assert_eq!(doc.append_text(top, bad), Err(NodeError::InvalidText));
        assert_eq!(
            doc.set_attribute(top, None, "k", bad),
            Err(NodeError::InvalidText)
        );
    }
    assert!(doc.children(top).is_empty(), "nothing was stored");

    // A comment is written verbatim, so it is held to what may appear
    // literally: no `--`, no trailing `-`, no control characters.
    for bad in ["a--b", "ends-", "\u{1}"] {
        assert_eq!(doc.append_comment(top, bad), Err(NodeError::InvalidText));
    }
    assert_eq!(doc.append_pi(top, "xml", "d"), Err(NodeError::InvalidName));
    assert_eq!(doc.append_pi(top, "t", "a?>b"), Err(NodeError::InvalidText));

    let _ = doc
        .append_text(top, "ok \u{1} \u{85}")
        .expect("expressible");
    let again = round_trip(&doc);
    assert_eq!(again.text(again.root()), "ok \u{1} \u{85}");
    assert!(doc.to_xml().starts_with("<?xml version=\"1.1\"?>"));
}

#[test]
fn a_control_character_in_a_namespace_uri_is_written_as_xml_11() {
    // Found by the mutate fuzz target: `declare_namespace` accepted the
    // URI, and the serialiser wrote `xmlns:r="&#1;"` with no 1.1
    // declaration, which no 1.0 parser reads.
    let (mut doc, top) = built();
    let _ = doc
        .declare_namespace(top, Some("r"), "\u{1}")
        .expect("expressible");
    assert!(doc.to_xml().starts_with("<?xml version=\"1.1\"?>"));
    let again = round_trip(&doc);
    assert_eq!(again.to_xml(), doc.to_xml());

    // The same URI reached through an element's name rather than a
    // declaration.
    let mut doc = oxml::tree::Document::empty();
    let root = doc.root();
    let _ = doc
        .append_element(root, Some("urn:\u{1}"), "e")
        .expect("expressible");
    assert!(doc.to_xml().starts_with("<?xml version=\"1.1\"?>"));
    let again = round_trip(&doc);
    assert_eq!(again.to_xml(), doc.to_xml());
}

// --- namespaces ---------------------------------------------------------

#[test]
fn a_namespaced_element_gets_a_declaration_when_written() {
    // `append_element(top, Some("urn:x"), "e")` serialised as `<e/>`:
    // the name was in a namespace nothing declared.
    let (mut doc, top) = built();
    let e = doc.append_element(top, Some("urn:x"), "e").expect("live");
    doc.set_attribute(e, Some("urn:y"), "a", "1").expect("live");

    let again = round_trip(&doc);
    let e2 = find(&again, "e");
    assert_eq!(
        again.element_name(e2).and_then(|n| n.namespace.as_deref()),
        Some("urn:x")
    );
    assert_eq!(again.attribute_ns(e2, Some("urn:y"), "a"), Some("1"));
    assert_eq!(again.attribute_ns(e2, None, "a"), None);
}

#[test]
fn a_declared_prefix_is_used_by_names_under_it() {
    let (mut doc, top) = built();
    let _ = doc
        .declare_namespace(top, Some("x"), "urn:x")
        .expect("live");
    let _ = doc.append_element(top, Some("urn:x"), "e").expect("live");
    assert_eq!(doc.to_xml(), "<top xmlns:x=\"urn:x\"><x:e/></top>");

    // Redeclaring a prefix on the same element replaces the binding.
    let _ = doc
        .declare_namespace(top, Some("x"), "urn:z")
        .expect("live");
    assert_eq!(doc.namespace_nodes(top).len(), 1);
    let out = doc.to_xml();
    assert!(out.contains("xmlns:x=\"urn:z\""), "{out}");
    // The element is still in urn:x, so it must be declared afresh.
    let again = round_trip(&doc);
    assert_eq!(
        again
            .element_name(find(&again, "e"))
            .and_then(|n| n.namespace.as_deref()),
        Some("urn:x")
    );
}

#[test]
fn a_default_namespace_is_undeclared_for_an_unqualified_child() {
    let mut doc = parse("<a xmlns=\"urn:d\"><b/></a>").expect("valid");
    let a = doc.root_element().expect("root element");
    let _ = doc.append_element(a, None, "plain").expect("live");
    let again = round_trip(&doc);
    assert_eq!(
        again
            .element_name(find(&again, "plain"))
            .map(|n| n.namespace.clone()),
        Some(None),
        "{}",
        doc.to_xml()
    );
}

#[test]
fn reparenting_out_of_a_declaration_scope_still_round_trips() {
    // `<m:b/>` moved out from under its `xmlns:m` was written with an
    // unbound prefix, which does not parse.
    let mut doc =
        parse("<doc><x xmlns:m=\"urn:m\"><m:b m:k=\"v\"/></x><y/></doc>")
            .expect("valid");
    let b = find(&doc, "b");
    let y = find(&doc, "y");
    doc.reparent(b, y).expect("live");

    let again = round_trip(&doc);
    let b2 = find(&again, "b");
    assert_eq!(
        again.element_name(b2).and_then(|n| n.namespace.as_deref()),
        Some("urn:m")
    );
    assert_eq!(again.attribute_ns(b2, Some("urn:m"), "k"), Some("v"));
    assert_eq!(
        again
            .element_name(again.parent(b2).expect("has parent"))
            .map(|n| n.local.as_str()),
        Some("y")
    );
}

#[test]
fn the_builder_can_make_namespaced_children() {
    let mut doc = Document::empty();
    let root = doc.root();
    let _ = doc
        .build(root, "top")
        .child_ns("urn:x", "e", |e| {
            let _ = e.attr_ns("urn:x", "a", "1").text("t");
        })
        .finish()
        .expect("live");
    let again = round_trip(&doc);
    let e = find(&again, "e");
    assert_eq!(
        again.element_name(e).and_then(|n| n.namespace.as_deref()),
        Some("urn:x")
    );
    assert_eq!(again.attribute_ns(e, Some("urn:x"), "a"), Some("1"));
}

// --- attributes compare by expanded name -------------------------------

#[test]
fn attributes_are_matched_by_expanded_name_not_prefix() {
    // Two prefixes bound to one URI produced two name ids, so
    // `remove_attribute(f, Some("urn:p"), "a")` found nothing on
    // `<f q:a="2"/>` and `set_attribute` appended a duplicate.
    let mut doc = parse(
        "<r xmlns:p=\"urn:p\" xmlns:q=\"urn:p\"><f p:a=\"1\"/><g q:a=\"2\"/></r>",
    )
    .expect("valid");
    let g = find(&doc, "g");
    assert_eq!(doc.attribute_ns(g, Some("urn:p"), "a"), Some("2"));

    doc.set_attribute(g, Some("urn:p"), "a", "3").expect("live");
    assert_eq!(doc.attribute_nodes(g).len(), 1, "replaced, not duplicated");
    assert_eq!(doc.attribute_ns(g, Some("urn:p"), "a"), Some("3"));

    assert_eq!(doc.remove_attribute(g, Some("urn:p"), "a"), Ok(true));
    assert!(doc.attribute_nodes(g).is_empty());
    let _ = round_trip(&doc);
}

// --- document order -----------------------------------------------------

/// Elements in the order `to_xml` writes them.
fn written_order(doc: &Document) -> Vec<String> {
    let xml = doc.to_xml();
    xml.split('<')
        .filter(|s| !s.is_empty() && !s.starts_with('/') && !s.starts_with('?'))
        .map(|s| {
            s.trim_end_matches("/>")
                .trim_end_matches('>')
                .split(' ')
                .next()
                .unwrap_or("")
                .to_owned()
        })
        .collect()
}

/// Elements sorted by their document-order key.
fn keyed_order(doc: &mut Document) -> Vec<String> {
    let order = doc.document_order().to_vec();
    let mut elements: Vec<(u32, String)> = doc
        .nodes()
        .filter(|n| doc.is_element(*n))
        .map(|n| {
            let key = order[n.index()];
            assert_ne!(key, u32::MAX, "a reachable element has a key");
            (key, doc.element_name(n).expect("element").local.clone())
        })
        .collect();
    elements.sort_unstable();
    elements.into_iter().map(|(_, n)| n).collect()
}

#[test]
fn document_order_follows_the_tree_not_the_arena() {
    let (mut doc, top) = built();
    let a = doc.append_element(top, None, "a").expect("live");
    let b = doc.append_element(top, None, "b").expect("live");
    let c = doc.append_element(b, None, "c").expect("live");
    assert!(!doc.is_order_dirty() || !doc.document_order().is_empty());

    doc.reparent(a, c).expect("live");
    assert!(doc.is_order_dirty(), "a move invalidates the order");
    assert_eq!(keyed_order(&mut doc), written_order(&doc));
    assert!(!doc.is_order_dirty());

    doc.remove(b).expect("live");
    let d = doc.append_element(top, None, "d").expect("live");
    doc.insert_at(top, 0, d).expect("live");
    let _ = doc.append_element(top, None, "e").expect("live");
    assert_eq!(keyed_order(&mut doc), written_order(&doc));
    assert_eq!(written_order(&doc), ["top", "d", "e"]);

    // Arena order is not document order after a move; `descendants_of`
    // is pre-order and is.
    let names: Vec<_> = doc
        .descendants_of(doc.root())
        .filter_map(|n| doc.element_name(n).map(|e| e.local.clone()))
        .collect();
    assert_eq!(names, ["top", "d", "e"]);
}

#[test]
fn an_unmutated_document_is_in_arena_order() {
    let doc = parse("<a><b/><c><d/></c></a>").expect("valid");
    assert!(!doc.is_order_dirty());
    for id in doc.nodes() {
        assert_eq!(doc.order_key(id), Some(id.index() as u32));
    }
}

// --- the new primitives ---------------------------------------------------

#[test]
fn insert_at_places_a_child_by_index() {
    let (mut doc, top) = built();
    let a = doc.append_element(top, None, "a").expect("live");
    let c = doc.append_element(top, None, "c").expect("live");
    let b = doc.append_element(a, None, "b").expect("live");
    doc.insert_at(top, 1, b).expect("live");
    assert_eq!(doc.children(top), [a, b, c]);
    assert_eq!(doc.parent(b), Some(top));
    assert!(doc.children(a).is_empty());
    // Past the end appends.
    doc.insert_at(top, 99, a).expect("live");
    assert_eq!(doc.children(top), [b, c, a]);
    assert_eq!(doc.insert_at(a, 0, top), Err(NodeError::WouldCycle));
    let _ = round_trip(&doc);
}

#[test]
fn set_text_replaces_in_place_and_text_content_borrows() {
    let (mut doc, top) = built();
    let t = doc.append_text(top, "one").expect("live");
    assert_eq!(doc.text_content(top), Some("one"));
    doc.set_text(t, "two").expect("a text node");
    assert_eq!(doc.text_content(top), Some("two"));
    assert_eq!(doc.text_content(t), Some("two"));
    assert_eq!(doc.set_text(top, "x"), Err(NodeError::NotText));
    assert_eq!(doc.set_text(t, "\0"), Err(NodeError::InvalidText));
    assert_eq!(
        doc.text_content(t),
        Some("two"),
        "a refusal changes nothing"
    );

    let _ = doc.append_element(top, None, "e").expect("live");
    assert_eq!(doc.text_content(top), None, "no longer a single text child");
    let empty = doc.append_element(top, None, "empty").expect("live");
    assert_eq!(doc.text_content(empty), Some(""));
}

#[test]
fn comments_and_processing_instructions_can_be_appended() {
    let (mut doc, top) = built();
    let _ = doc.append_comment(top, " note ").expect("live");
    let _ = doc.append_pi(top, "target", "data").expect("live");
    let _ = doc.append_pi(top, "bare", "").expect("live");
    let _ = doc
        .append_comment(doc.root(), "trailing")
        .expect("root is a container");
    assert_eq!(
        doc.to_xml(),
        "<top><!-- note --><?target data?><?bare?></top><!--trailing-->"
    );
    let _ = round_trip(&doc);
}

#[test]
fn detach_unlinks_without_freeing() {
    let (mut doc, top) = built();
    let a = doc.append_element(top, None, "a").expect("live");
    let _ = doc.append_text(a, "kept").expect("live");
    doc.detach(a).expect("live");
    assert!(doc.children(top).is_empty());
    assert_eq!(doc.parent(a), None);
    assert_eq!(doc.text(a), "kept", "the subtree is intact");
    assert_eq!(doc.to_xml(), "<top/>", "and not written");
    let _ = doc.document_order();
    assert_eq!(doc.order_key(a), None, "and has no document position");

    doc.reparent(a, top)
        .expect("a detached subtree can come back");
    assert_eq!(doc.to_xml(), "<top><a>kept</a></top>");
    doc.detach(a).expect("live");
    doc.compact();
    assert_eq!(doc.text(a), "kept", "compaction keeps detached subtrees");
    doc.remove(a).expect("a detached subtree can be removed");
    assert_eq!(doc.len(), doc.nodes().count());
}

#[test]
fn compact_drops_the_garbage_and_changes_nothing_visible() {
    let mut doc = parse("<r><a/><b x=\"1\"/><c/></r>").expect("valid");
    let r = doc.root_element().expect("root element");
    let b = find(&doc, "b");
    for _ in 0..100 {
        let e = doc.append_element(r, None, "e").expect("live");
        doc.remove(e).expect("live");
    }
    doc.set_attribute(b, None, "y", "2").expect("live");
    let before = doc.to_xml();
    let ids: Vec<_> = doc.nodes().collect();

    doc.compact();

    assert_eq!(doc.to_xml(), before);
    assert_eq!(doc.nodes().collect::<Vec<_>>(), ids, "ids are stable");
    assert_eq!(doc.attribute(b, "y"), Some("2"));
    assert!(!doc.is_order_dirty());
    assert_eq!(keyed_order(&mut doc), written_order(&doc));
}

#[test]
fn a_cloned_document_is_independent() {
    let (mut doc, top) = built();
    let a = doc.append_element(top, None, "a").expect("live");
    let copy = doc.clone();
    doc.remove(a).expect("live");
    assert_eq!(copy.to_xml(), "<top><a/></top>");
    assert_eq!(doc.to_xml(), "<top/>");
}
