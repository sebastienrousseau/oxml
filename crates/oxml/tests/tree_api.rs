// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The tree API.
//!
//! Every accessor is reached through a parsed document, so these pin
//! the contract a caller navigating a tree by hand depends on —
//! including what each one does when asked about a node of the wrong
//! kind, which is where an arena API most easily misbehaves.

use oxml::{ExpandedName, NodeKind, parse};

const DOC: &str = "\
<r xmlns:m=\"urn:m\" id=\"top\">\
<!-- c -->\
<a x=\"1\" y=\"2\">text</a>\
<m:b>ns</m:b>\
<empty/>\
</r>";

fn doc() -> oxml::Document {
    parse(DOC).expect("well-formed")
}

/// The element with the given local name.
fn find(d: &oxml::Document, local: &str) -> oxml::NodeId {
    d.descendants()
        .find(|n| d.element_name(*n).is_some_and(|e| e.local == local))
        .unwrap_or_else(|| panic!("no element named {local}"))
}

#[test]
fn a_node_id_exposes_its_arena_index() {
    let d = doc();
    assert_eq!(d.root().index(), 0, "the root is the first node");
    for id in d.descendants() {
        assert!(id.index() < d.len());
    }
}

#[test]
fn the_root_is_the_document_node_not_the_root_element() {
    // Conflating the two is the classic XPath tree mistake: `/` and
    // `/r` are different nodes.
    let d = doc();
    assert!(!d.is_element(d.root()));
    let re = d.root_element().expect("a root element");
    assert_ne!(re, d.root());
    assert!(d.is_element(re));
    assert_eq!(d.element_name(re).map(|n| n.local.as_str()), Some("r"));
}

#[test]
fn parent_links_lead_back_to_the_root() {
    let d = doc();
    assert_eq!(d.parent(d.root()), None, "the root has no parent");
    let a = find(&d, "a");
    let mut hops = 0;
    let mut cur = Some(a);
    while let Some(n) = cur {
        cur = d.parent(n);
        hops += 1;
        assert!(hops < 100, "parent links must terminate");
    }
    assert_eq!(hops, 3, "a -> r -> document");
}

#[test]
fn children_excludes_attributes() {
    // Attributes are nodes and are reachable, but they are not
    // children — that distinction is the whole attribute axis.
    let d = doc();
    let r = d.root_element().expect("root element");
    let child_names: Vec<String> = d
        .children(r)
        .iter()
        .filter_map(|c| d.element_name(*c))
        .map(|n| n.local.clone())
        .collect();
    assert_eq!(child_names, ["a", "b", "empty"]);
    assert!(
        !d.children(r)
            .iter()
            .any(|c| matches!(d.kind(*c), Some(NodeKind::Attr(_)))),
        "an attribute appeared among the children"
    );
    assert!(d.children(find(&d, "empty")).is_empty());
}

#[test]
fn attribute_lookup_is_by_local_name() {
    let d = doc();
    let a = find(&d, "a");
    assert_eq!(d.attribute(a, "x"), Some("1"));
    assert_eq!(d.attribute(a, "y"), Some("2"));
    assert_eq!(d.attribute(a, "absent"), None);
    // Asking a non-element for an attribute is None, not a panic.
    assert_eq!(d.attribute(d.root(), "x"), None);
}

#[test]
fn attributes_and_attribute_nodes_agree() {
    let d = doc();
    let a = find(&d, "a");
    assert_eq!(d.attributes(a).len(), 2);
    assert_eq!(d.attribute_nodes(a).len(), 2);
    assert!(d.attributes(find(&d, "empty")).is_empty());
    assert!(d.attribute_nodes(d.root()).is_empty());

    for id in d.attribute_nodes(a) {
        assert!(matches!(d.kind(*id), Some(NodeKind::Attr(_))));
        assert_eq!(d.parent(*id), Some(a), "an attribute knows its owner");
    }
}

#[test]
fn text_concatenates_descendant_text() {
    let d = doc();
    assert_eq!(d.text(find(&d, "a")), "text");
    assert_eq!(d.text(find(&d, "empty")), "");
    // The root element's text is every descendant's text in order.
    let all = d.text(d.root_element().expect("root element"));
    assert!(all.contains("text"), "{all}");
    assert!(all.contains("ns"), "{all}");
    assert!(!all.contains('c'), "a comment is not text: {all}");
}

#[test]
fn element_name_carries_the_namespace_when_there_is_one() {
    let d = doc();
    let b = find(&d, "b");
    let name = d.element_name(b).expect("a name");
    assert_eq!(name.local, "b");
    assert_eq!(name.namespace.as_deref(), Some("urn:m"));

    let a = d.element_name(find(&d, "a")).expect("a name");
    assert_eq!(a.namespace, None);

    assert!(
        d.element_name(d.root()).is_none(),
        "the document has no name"
    );
}

#[test]
fn kind_reports_each_node_kind() {
    let d = doc();
    let mut seen_comment = false;
    let mut seen_text = false;
    let mut seen_attr = false;
    for id in d.descendants() {
        match d.kind(id) {
            Some(NodeKind::Comment(c)) => {
                assert!(c.contains('c'));
                seen_comment = true;
            }
            Some(NodeKind::Text(_)) => seen_text = true,
            Some(NodeKind::Attr(_)) => seen_attr = true,
            _ => {}
        }
    }
    assert!(seen_comment && seen_text && seen_attr);
}

#[test]
fn descendants_visits_every_node_exactly_once() {
    let d = doc();
    let ids: Vec<_> = d.descendants().collect();
    assert_eq!(ids.len(), d.len());
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "a node was visited twice");
}

#[test]
fn len_and_is_empty_describe_the_arena() {
    let minimal = parse("<a/>").expect("well-formed");
    assert!(minimal.len() >= 2, "a document node and an element");
    assert!(!minimal.is_empty(), "it has a root element");

    let d = doc();
    assert!(d.len() > minimal.len());
    assert!(!d.is_empty());
}

#[test]
fn expanded_names_can_be_built_locally_and_qualified() {
    let local = ExpandedName::local("a");
    assert_eq!(local.local, "a");
    assert_eq!(local.namespace, None);

    let qualified = ExpandedName::qualified("urn:m", "b");
    assert_eq!(qualified.local, "b");
    assert_eq!(qualified.namespace.as_deref(), Some("urn:m"));
    assert_ne!(local, qualified);
}

#[test]
fn accessors_are_total_over_every_node_in_the_arena() {
    // No accessor may panic for any node the tree itself hands out.
    let d = doc();
    for id in d.descendants() {
        let _ = d.kind(id);
        let _ = d.parent(id);
        let _ = d.children(id);
        let _ = d.is_element(id);
        let _ = d.element_name(id);
        let _ = d.attributes(id);
        let _ = d.attribute_nodes(id);
        let _ = d.attribute(id, "x");
        let _ = d.text(id);
    }
}
