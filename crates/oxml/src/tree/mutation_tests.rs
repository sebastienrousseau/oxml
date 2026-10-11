// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Unit tests for the mutation primitives.

use super::*;

#[test]
fn a_document_can_be_built_from_nothing() {
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("root is live");
    let b = doc.append_element(a, None, "b").expect("a is live");
    let _ = doc.append_text(b, "hello").expect("b is live");

    assert_eq!(doc.children(root), [a]);
    assert_eq!(doc.children(a), [b]);
    assert_eq!(doc.text(root), "hello");
}

#[test]
fn children_stay_contiguous_and_ordered_as_they_are_appended() {
    // A block moves when it is full and grows in place otherwise.
    // Order and contents must survive both, or reads silently see a
    // stale block.
    let mut doc = Document::empty();
    let root = doc.root();
    let parent = doc.append_element(root, None, "p").expect("live");
    let kids: Vec<NodeId> = (0..40)
        .map(|i| {
            if i % 2 == 0 {
                doc.append_element(parent, None, "e").expect("live")
            } else {
                doc.append_text(parent, "t").expect("live")
            }
        })
        .collect();
    assert_eq!(doc.children(parent), kids.as_slice());
}

#[test]
fn appending_does_not_grow_the_arena_quadratically() {
    let mut doc = Document::empty();
    let root = doc.root();
    let parent = doc.append_element(root, None, "p").expect("live");
    for _ in 0..1_000 {
        let _ = doc.append_element(parent, None, "e").expect("live");
    }
    assert!(
        doc.child_ids.len() < 5_000,
        "{} arena entries for 1,000 children",
        doc.child_ids.len()
    );
}

#[test]
fn a_removed_node_leaves_its_identifier_stale() {
    // The reason NodeId carries a generation at all.
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    assert!(doc.resolve(a).is_some());

    doc.remove(a).expect("a is live");

    assert!(doc.resolve(a).is_none(), "the identifier must not resolve");
    assert_eq!(
        doc.remove(a),
        Err(NodeError::Stale),
        "and removing twice is an error"
    );
    assert!(
        doc.children(root).is_empty(),
        "and the parent must not still list it"
    );
}

#[test]
fn a_walk_does_not_visit_removed_nodes() {
    // Found by the mutation fuzz target, not by review. Removal
    // bumps a slot's generation, but `nodes` mints identifiers from
    // each slot's *current* generation -- so a removed node minted
    // cleanly from its own bumped generation and looked live.
    let mut doc = Document::empty();
    let root = doc.root();
    let top = doc.append_element(root, None, "r").expect("live");
    let a = doc.append_element(top, None, "a").expect("live");
    let _deep = doc.append_element(a, None, "b").expect("live");
    assert_eq!(doc.nodes().count(), 4);

    doc.remove(a).expect("live");

    assert_eq!(doc.nodes().count(), 2, "root and r only");
    assert!(
        !doc.nodes().any(|d| d.index() == a.index()),
        "a removed node must not be walked"
    );

    // And the invariant the fuzz target asserts: every child names
    // its parent.
    for id in doc.nodes() {
        for child in doc.children(id) {
            assert_eq!(doc.parent(*child), Some(id), "links disagree");
        }
    }
}

#[test]
fn removing_a_subtree_invalidates_every_descendant() {
    // Removing only the top of a subtree would leave descendants
    // resolving through identifiers whose parent is gone.
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    let b = doc.append_element(a, None, "b").expect("live");
    let t = doc.append_text(b, "deep").expect("live");
    doc.set_attribute(b, None, "k", "v").expect("live");
    let attr = doc.attribute_nodes(b)[0];
    let ns = doc.declare_namespace(b, Some("p"), "urn:p").expect("live");

    doc.remove(a).expect("live");

    for (name, id) in
        [("a", a), ("b", b), ("text", t), ("attr", attr), ("ns", ns)]
    {
        assert!(doc.resolve(id).is_none(), "{name} should be stale");
    }
    assert_eq!(doc.len(), 1, "only the root is left");
    assert_eq!(doc.free_slots.len(), 5, "each slot listed once");
}

#[test]
fn removing_one_child_leaves_its_siblings_alone() {
    let mut doc = Document::empty();
    let root = doc.root();
    let p = doc.append_element(root, None, "p").expect("live");
    let one = doc.append_element(p, None, "one").expect("live");
    let two = doc.append_element(p, None, "two").expect("live");
    let three = doc.append_element(p, None, "three").expect("live");

    doc.remove(two).expect("live");

    assert_eq!(doc.children(p), [one, three]);
    assert!(doc.resolve(one).is_some());
    assert!(doc.resolve(three).is_some());
}

#[test]
fn appending_to_a_removed_parent_is_an_error_not_a_panic() {
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    doc.remove(a).expect("live");

    assert_eq!(doc.append_element(a, None, "b"), Err(NodeError::Stale));
    assert_eq!(doc.append_text(a, "t"), Err(NodeError::Stale));
    assert_eq!(doc.append_comment(a, "c"), Err(NodeError::Stale));
    assert_eq!(doc.append_pi(a, "p", ""), Err(NodeError::Stale));
    assert_eq!(doc.detach(a), Err(NodeError::Stale));
    assert_eq!(doc.set_text(a, "t"), Err(NodeError::Stale));
    assert_eq!(
        doc.declare_namespace(a, None, "urn:x"),
        Err(NodeError::Stale)
    );
}

#[test]
fn a_built_document_serialises_and_reparses_to_the_same_tree() {
    // The join between the mutation path and the serialiser: a
    // document built by hand must be a fixed point just as a parsed
    // one is.
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    let b = doc.append_element(a, None, "b").expect("live");
    let _ = doc.append_text(b, "p & q").expect("live");

    let once = doc.to_xml();
    let reparsed = crate::parse(&once).expect("what we built must parse");
    assert_eq!(reparsed.to_xml(), once, "built: {once}");
    assert_eq!(reparsed.text(reparsed.root()), "p & q");
}

#[test]
fn text_is_stored_verbatim_and_escaped_only_on_the_way_out() {
    // A caller passes characters, not markup. If `&` were stored
    // as written and emitted as written, the reparse would read an
    // entity reference.
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    let _ = doc.append_text(a, "x & y < z").expect("live");

    assert_eq!(doc.text(root), "x & y < z");
    let out = doc.to_xml();
    assert!(out.contains("&amp;"), "{out}");
    assert!(out.contains("&lt;"), "{out}");
    assert_eq!(
        crate::parse(&out).expect("reparses").text(doc.root()),
        "x & y < z"
    );
}

#[test]
fn names_are_interned_rather_than_repeated() {
    let mut doc = Document::empty();
    let root = doc.root();
    let holder = doc.append_element(root, None, "holder").expect("live");
    let before = doc.names.len();
    for _ in 0..10 {
        let _ = doc.append_element(holder, None, "same").expect("live");
    }
    assert_eq!(doc.names.len(), before + 1, "one name, ten elements");
}

#[test]
fn expanded_text_entries_are_reused_after_removal() {
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    let t = doc.append_text(a, "gone").expect("live");
    let entries = doc.expanded.len();
    doc.remove(t).expect("live");
    assert_eq!(doc.free_expanded.len(), 1);
    let _ = doc.append_text(a, "back").expect("live");
    assert_eq!(doc.expanded.len(), entries, "the entry was reused");
    assert_eq!(doc.text(a), "back");
}

fn built() -> (Document, NodeId, NodeId) {
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    (doc, root, a)
}

#[test]
fn an_attribute_can_be_set_and_read_back() {
    let (mut doc, _root, a) = built();
    doc.set_attribute(a, None, "id", "one").expect("live");
    assert_eq!(doc.attribute(a, "id"), Some("one"));
    assert_eq!(doc.attribute_ns(a, None, "id"), Some("one"));
    assert_eq!(doc.attribute_ns(a, Some("urn:x"), "id"), None);
}

#[test]
fn setting_the_same_name_twice_replaces_rather_than_duplicates() {
    // XML forbids duplicate attribute names on an element. If this
    // appended instead of replacing, the document would serialise
    // to something that will not parse.
    let (mut doc, _root, a) = built();
    doc.set_attribute(a, None, "id", "one").expect("live");
    let entries = doc.expanded.len();
    doc.set_attribute(a, None, "id", "two").expect("live");

    assert_eq!(doc.attribute(a, "id"), Some("two"));
    assert_eq!(doc.attribute_nodes(a).len(), 1, "one attribute, not two");
    assert_eq!(doc.expanded.len(), entries, "replaced in place");

    let xml = doc.to_xml();
    assert!(crate::parse(&xml).is_ok(), "must stay well-formed: {xml}");
}

#[test]
fn several_attributes_survive_the_arena_relocation() {
    let (mut doc, _root, a) = built();
    for (n, v) in [
        ("one", "1"),
        ("two", "2"),
        ("three", "3"),
        ("four", "4"),
        ("five", "5"),
    ] {
        doc.set_attribute(a, None, n, v).expect("live");
    }
    assert_eq!(doc.attribute_nodes(a).len(), 5);
    assert_eq!(doc.attribute(a, "one"), Some("1"));
    assert_eq!(doc.attribute(a, "five"), Some("5"));
}

#[test]
fn removing_an_attribute_reports_whether_there_was_one() {
    let (mut doc, _root, a) = built();
    doc.set_attribute(a, None, "id", "one").expect("live");

    assert_eq!(doc.remove_attribute(a, None, "id"), Ok(true));
    assert_eq!(doc.attribute(a, "id"), None);
    assert_eq!(
        doc.remove_attribute(a, None, "id"),
        Ok(false),
        "removing it again is not an error, just nothing to do"
    );
    assert_eq!(
        doc.remove_attribute(a, None, "never-existed"),
        Ok(false),
        "a name the document has never interned is not an error either"
    );
}

#[test]
fn attributes_need_an_element() {
    let (mut doc, root, a) = built();
    let text = doc.append_text(a, "t").expect("live");
    assert_eq!(
        doc.set_attribute(text, None, "id", "x"),
        Err(NodeError::NotAnElement)
    );
    assert_eq!(
        doc.set_attribute(root, None, "id", "x"),
        Err(NodeError::NotAnElement),
        "the document root is not an element"
    );
    assert_eq!(
        doc.declare_namespace(root, None, "urn:x"),
        Err(NodeError::NotAnElement)
    );
}

#[test]
fn attributes_of_a_removed_element_are_stale() {
    let (mut doc, _root, a) = built();
    doc.remove(a).expect("live");
    assert_eq!(doc.set_attribute(a, None, "id", "x"), Err(NodeError::Stale));
    assert_eq!(doc.remove_attribute(a, None, "id"), Err(NodeError::Stale));
}

#[test]
fn a_subtree_can_be_moved_between_parents() {
    let mut doc = Document::empty();
    let root = doc.root();
    let d = doc.append_element(root, None, "d").expect("live");
    let from = doc.append_element(d, None, "from").expect("live");
    let to = doc.append_element(d, None, "to").expect("live");
    let moved = doc.append_element(from, None, "moved").expect("live");
    let _ = doc.append_text(moved, "payload").expect("live");

    doc.reparent(moved, to).expect("both live, no cycle");

    assert!(doc.children(from).is_empty(), "left the old parent");
    assert_eq!(doc.children(to), [moved], "arrived at the new one");
    assert_eq!(doc.parent(moved), Some(to), "and knows its parent");
    assert_eq!(doc.text(to), "payload", "with its subtree intact");
}

#[test]
fn a_node_cannot_be_moved_into_its_own_subtree() {
    // Without this check the tree would contain a loop, and every
    // walker here follows children until they run out.
    let mut doc = Document::empty();
    let root = doc.root();
    let outer = doc.append_element(root, None, "outer").expect("live");
    let inner = doc.append_element(outer, None, "inner").expect("live");
    let deeper = doc.append_element(inner, None, "deeper").expect("live");

    assert_eq!(doc.reparent(outer, inner), Err(NodeError::WouldCycle));
    assert_eq!(doc.reparent(outer, deeper), Err(NodeError::WouldCycle));
    assert_eq!(doc.reparent(outer, outer), Err(NodeError::WouldCycle));

    // And the tree is untouched by the refusal.
    assert_eq!(doc.children(root), [outer]);
    assert_eq!(doc.children(outer), [inner]);
}

#[test]
fn a_refused_move_leaves_the_document_walkable() {
    // The property the cycle check protects: after a refusal, a
    // full walk still terminates.
    let mut doc = Document::empty();
    let root = doc.root();
    let outer = doc.append_element(root, None, "outer").expect("live");
    let inner = doc.append_element(outer, None, "inner").expect("live");
    let _ = doc.reparent(outer, inner);

    assert_eq!(doc.nodes().count(), doc.len());
    assert_eq!(doc.descendants_of(root).count() + 1, doc.len());
    assert!(doc.to_xml().contains("<outer>"), "{}", doc.to_xml());
}

#[test]
fn moving_to_a_removed_parent_is_stale() {
    let mut doc = Document::empty();
    let root = doc.root();
    let d = doc.append_element(root, None, "d").expect("live");
    let a = doc.append_element(d, None, "a").expect("live");
    let b = doc.append_element(d, None, "b").expect("live");
    doc.remove(b).expect("live");
    assert_eq!(doc.reparent(a, b), Err(NodeError::Stale));
    assert_eq!(doc.reparent(b, a), Err(NodeError::Stale));
}

#[test]
fn a_second_root_element_is_refused() {
    // XML permits exactly one. Building two silently produced a
    // document that serialised and then failed to reparse with
    // `TrailingContent` -- found by a test that meant to check
    // something else.
    let mut doc = Document::empty();
    let root = doc.root();
    let first = doc.append_element(root, None, "one").expect("live");

    assert_eq!(
        doc.append_element(root, None, "two"),
        Err(NodeError::RootElementExists)
    );

    // Whereas nesting is fine, and so is a comment or text beside
    // the root element at document level.
    let inner = doc.append_element(first, None, "inner");
    assert!(inner.is_ok(), "{inner:?}");
    assert!(doc.append_text(root, " ").is_ok(), "text at document level");

    let xml = doc.to_xml();
    assert!(crate::parse(&xml).is_ok(), "must reparse: {xml}");
}

#[test]
fn a_moved_document_still_round_trips() {
    let mut doc = Document::empty();
    let root = doc.root();
    let doc_el = doc.append_element(root, None, "doc").expect("live");
    let from = doc.append_element(doc_el, None, "from").expect("live");
    let to = doc.append_element(doc_el, None, "to").expect("live");
    let moved = doc.append_element(from, None, "moved").expect("live");
    doc.set_attribute(moved, None, "k", "v & w").expect("live");
    doc.reparent(moved, to).expect("live");

    let once = doc.to_xml();
    let reparsed = crate::parse(&once).expect("must parse: {once}");
    assert_eq!(reparsed.to_xml(), once);
    assert!(once.contains("&amp;"), "attribute escaped: {once}");
}

#[test]
fn recycled_slots_do_not_grow_arena_and_reject_stale_handles() {
    let mut doc = Document::empty();
    let root = doc.root();
    let first = doc.append_element(root, None, "item").expect("live");
    let first_index = first.index();
    let initial_slots = doc.nodes.len();

    doc.remove(first).expect("live");
    assert!(
        doc.resolve(first).is_none(),
        "removed node is not resolvable"
    );

    let second = doc.append_element(root, None, "item2").expect("live");
    assert_eq!(second.index(), first_index, "reused recycled slot");
    assert_ne!(second.generation, first.generation, "generation advanced");
    assert_eq!(doc.nodes.len(), initial_slots, "arena size did not grow");

    assert!(
        doc.resolve(first).is_none(),
        "stale generation does not resolve"
    );
    assert_eq!(
        doc.element_name(second).map(|n| n.local.as_str()),
        Some("item2")
    );
}

#[test]
fn len_counts_live_nodes_only() {
    let mut doc = Document::empty();
    let root = doc.root();
    let a = doc.append_element(root, None, "a").expect("live");
    let _ = doc.append_text(a, "t").expect("live");
    assert_eq!(doc.len(), 3);
    doc.remove(a).expect("live");
    assert_eq!(doc.len(), 1);
    assert!(doc.is_empty());
}

#[test]
fn the_parsed_prefix_is_kept_when_in_scope() {
    let mut doc =
        crate::parse("<r xmlns:p=\"urn:p\"><p:a/></r>").expect("valid");
    let r = doc.root_element().expect("root element");
    let _ = doc.append_element(r, Some("urn:p"), "b").expect("live");
    assert_eq!(doc.to_xml(), "<r xmlns:p=\"urn:p\"><p:a/><p:b/></r>");
}

#[test]
fn generated_prefixes_avoid_those_in_scope() {
    let mut doc = crate::parse("<r xmlns:ns1=\"urn:taken\"/>").expect("valid");
    let r = doc.root_element().expect("root element");
    let e = doc.append_element(r, Some("urn:x"), "e").expect("live");
    doc.set_attribute(e, Some("urn:y"), "a", "1").expect("live");
    assert_eq!(
        doc.to_xml(),
        "<r xmlns:ns1=\"urn:taken\"><ns2:e xmlns:ns2=\"urn:x\" \
         xmlns:ns3=\"urn:y\" ns3:a=\"1\"/></r>"
    );
}
