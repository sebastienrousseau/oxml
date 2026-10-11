#![no_main]
//! A random sequence of mutations must leave a coherent tree.
//!
//! The mutation API relocates blocks in three shared arenas and bumps
//! slot generations on removal. Each operation is unit-tested; what
//! this asserts is that no *sequence* of them can produce a tree that
//! parent and child links disagree about, that a walker cannot
//! terminate on, that a slot is freed twice for, or that serialises
//! to something the parser rejects.
//!
//! Those are the failures a panic-only target would miss: none of them
//! crashes, and all of them are wrong.

use libfuzzer_sys::fuzz_target;
use oxml::tree::{Document, NodeId, NodeKind};

/// Bounded so the target spends its time on sequences rather than on
/// one enormous document, and so a walk cannot be mistaken for a hang.
const MAX_OPS: usize = 256;

/// Names and namespaces to draw from, some repeated under two names.
const NAMES: [&str; 4] = ["e", "f", "ns-name", "r"];
const NAMESPACES: [Option<&str>; 3] = [None, Some("urn:a"), Some("urn:b")];
const TEXTS: [&str; 4] = ["t", "a & b", "caf\u{85}", "\u{1}"];

fuzz_target!(|data: &[u8]| {
    let mut doc = Document::empty();
    let root = doc.root();
    // Every document needs its one root element before anything can
    // be appended below it.
    let Ok(top) = doc.append_element(root, None, "r") else {
        return;
    };
    let mut live: Vec<NodeId> = vec![top];

    for chunk in data.chunks(4).take(MAX_OPS) {
        let op = chunk[0] % 13;
        let pick = |i: usize| chunk.get(i).copied().unwrap_or(0) as usize;
        let target = live[pick(1) % live.len()];
        // The root is a destination only: appending text under it
        // is legal for the API but not well-formed XML.
        let other = match pick(2) % (live.len() + 1) {
            0 => root,
            n => live[n - 1],
        };
        let name = NAMES[pick(3) % NAMES.len()];
        let namespace = NAMESPACES[pick(3) % NAMESPACES.len()];
        let text = TEXTS[pick(3) % TEXTS.len()];

        match op {
            0 | 1 => {
                if let Ok(id) = doc.append_element(target, namespace, name) {
                    live.push(id);
                }
            }
            2 => {
                if let Ok(id) = doc.append_text(target, text) {
                    live.push(id);
                }
            }
            3 => {
                // Removing the one root element would leave a document
                // that cannot serialise to well-formed XML, which is
                // the caller's business rather than a defect here.
                // Attribute and namespace nodes are removable too.
                if target != top {
                    let _ = doc.remove(target);
                }
            }
            4 => {
                let _ = doc.set_attribute(target, namespace, name, text);
                // Attribute nodes join the pool, so later operations
                // can remove, move or detach them.
                live.extend_from_slice(doc.attribute_nodes(target));
            }
            5 => {
                // A refusal is a valid outcome; what must not happen
                // is a cycle being created or a second root element.
                let _ = doc.reparent(target, other);
            }
            6 => {
                if let Ok(id) = doc.declare_namespace(target, Some(name), text)
                {
                    live.push(id);
                }
            }
            7 => {
                let _ = doc.insert_at(other, pick(3), target);
            }
            8 => {
                let _ = doc.set_text(target, text);
            }
            9 => {
                if target != top {
                    let _ = doc.detach(target);
                }
            }
            10 => {
                let _ = doc.remove_attribute(target, namespace, name);
            }
            11 => {
                if let Ok(id) = doc.append_comment(target, text) {
                    live.push(id);
                }
            }
            _ => doc.compact(),
        }

        // Identifiers of removed nodes must stop resolving, or a later
        // operation would address whatever occupies the slot.
        live.retain(|n| doc.kind(*n).is_some());
        live.dedup();
    }

    check_links(&doc);
    check_order(&mut doc);
    check_serialisation(&doc);
});

/// Parent and child links agree, nothing is listed twice, and only
/// containers have children.
fn check_links(doc: &Document) {
    let mut seen = 0usize;
    for id in doc.nodes() {
        seen += 1;
        let kind = doc.kind(id).expect("nodes() yields live nodes only");
        // 1. A node with children is the root or an element. A child
        //    under a text node was once stored and silently dropped.
        let kids = doc.children(id);
        assert!(
            kids.is_empty()
                || matches!(kind, NodeKind::Root | NodeKind::Element { .. }),
            "a {kind:?} has children"
        );
        // 2. Every yielded node's parent resolves, and lists it.
        if let Some(parent) = doc.parent(id) {
            assert!(doc.kind(parent).is_some(), "parent of {id:?} is gone");
            let listed = doc.children(parent).contains(&id)
                || doc.attribute_nodes(parent).contains(&id)
                || doc.namespace_nodes(parent).contains(&id);
            assert!(listed, "{parent:?} does not list {id:?}");
        }
        // 3. Parent and child links agree.
        for child in kids {
            assert_eq!(doc.parent(*child), Some(id), "links disagree");
        }
        // 4. No node lists the same child twice. A bad block copy would
        //    duplicate an entry, which every walker would then visit
        //    twice.
        for (i, a) in kids.iter().enumerate() {
            assert!(
                !kids[i + 1..].contains(a),
                "a child appears twice under one parent"
            );
        }
    }
    // 5. `len` counts live nodes, which means every freed slot is on
    //    the free list exactly once.
    assert_eq!(seen, doc.len(), "len() disagrees with nodes()");
}

/// Document order is a permutation of the reachable nodes consistent
/// with the children order.
fn check_order(doc: &mut Document) {
    let order = doc.document_order().to_vec();
    assert!(!doc.is_order_dirty());
    let mut keys: Vec<u32> = Vec::new();
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let key = order[id.index()];
        assert_ne!(key, u32::MAX, "a reachable node has no position");
        keys.push(key);
        let kids = doc.children(id);
        for pair in kids.windows(2) {
            assert!(
                order[pair[0].index()] < order[pair[1].index()],
                "siblings out of order"
            );
        }
        for child in kids {
            assert!(key < order[child.index()], "a child before its parent");
        }
        stack.extend(kids.iter().rev());
    }
    // Every reachable node, and only those, counted from the root's
    // perspective; `descendants_of` must agree.
    assert_eq!(keys.len(), doc.descendants_of(doc.root()).count() + 1);
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(
        keys.len(),
        doc.descendants_of(doc.root()).count() + 1,
        "positions are not distinct"
    );
}

/// Whatever was built serialises to something the parser accepts and
/// that is a fixed point.
///
/// This is the property a caller actually depends on. It is asserted
/// only when the document is well-formed at the top level: the API
/// permits text beside the root element, which is not XML.
fn check_serialisation(doc: &Document) {
    let xml = doc.to_xml();
    let well_formed_top = doc.root_element().is_some()
        && doc.children(doc.root()).iter().all(|c| {
            matches!(
                doc.kind(*c),
                Some(
                    NodeKind::Element { .. }
                        | NodeKind::Comment(_)
                        | NodeKind::ProcessingInstruction { .. }
                )
            )
        });
    if !well_formed_top {
        return;
    }
    let reparsed = oxml::parse(&xml)
        .unwrap_or_else(|e| panic!("output does not parse: {e}\n{xml}"));
    assert_eq!(
        reparsed.to_xml(),
        xml,
        "serialisation is not a fixed point"
    );
}
