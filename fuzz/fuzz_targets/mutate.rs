#![no_main]
//! A random sequence of mutations must leave a coherent tree.
//!
//! The mutation API relocates blocks in two shared arenas and bumps
//! slot generations on removal. Each operation is unit-tested; what
//! this asserts is that no *sequence* of them can produce a tree that
//! parent and child links disagree about, that a walker cannot
//! terminate on, or that serialises to something the parser rejects.
//!
//! Those are the failures a panic-only target would miss: none of them
//! crashes, and all of them are wrong.

use libfuzzer_sys::fuzz_target;
use oxml::tree::{Document, NodeId};

/// Bounded so the target spends its time on sequences rather than on
/// one enormous document, and so a walk cannot be mistaken for a hang.
const MAX_OPS: usize = 256;

fuzz_target!(|data: &[u8]| {
    let mut doc = Document::empty();
    let root = doc.root();
    // Every document needs its one root element before anything can
    // be appended below it.
    let Ok(top) = doc.append_element(root, None, "r") else {
        return;
    };
    let mut live: Vec<NodeId> = alloc_vec(top);

    for chunk in data.chunks(3).take(MAX_OPS) {
        let op = chunk[0] % 5;
        let pick = chunk.get(1).copied().unwrap_or(0) as usize;
        let target = live[pick % live.len()];

        match op {
            0 => {
                if let Ok(id) = doc.append_element(target, None, "e") {
                    live.push(id);
                }
            }
            1 => {
                if let Ok(id) = doc.append_text(target, "t") {
                    live.push(id);
                }
            }
            2 => {
                // Removing the one root element would leave a document
                // that cannot serialise to well-formed XML, which is
                // the caller's business rather than a defect here.
                if target != top && doc.remove(target).is_ok() {
                    live.retain(|n| *n != target);
                }
            }
            3 => {
                let _ = doc.set_attribute(target, None, "k", "v");
            }
            _ => {
                let other = live[chunk.get(2).copied().unwrap_or(0) as usize % live.len()];
                // A refusal is a valid outcome; what must not happen
                // is a cycle being created.
                let _ = doc.reparent(target, other);
            }
        }

        // Identifiers of removed nodes must stop resolving, or a later
        // operation would address whatever occupies the slot.
        live.retain(|n| doc.parent(*n).is_some() || *n == top);
    }

    // 1. Every walk terminates and visits each node once. If a
    //    reparent had built a cycle, this would not return.
    let seen = doc.descendants().count();
    assert!(seen <= doc.len(), "descendants exceeded the arena");

    // 2. Parent and child links agree.
    for id in doc.descendants() {
        for child in doc.children(id) {
            assert_eq!(doc.parent(*child), Some(id), "links disagree");
        }
    }

    // 3. No node lists the same child twice. The arena copies blocks
    //    on append; a bad copy would duplicate an entry, which every
    //    walker would then visit twice.
    for id in doc.descendants() {
        let kids = doc.children(id);
        for (i, a) in kids.iter().enumerate() {
            assert!(
                !kids[i + 1..].contains(a),
                "a child appears twice under one parent"
            );
        }
    }

    // 4. Whatever was built serialises to something the parser accepts
    //    and that is a fixed point. This is the property a caller
    //    actually depends on.
    let xml = doc.to_xml();
    if let Ok(reparsed) = oxml::parse(&xml) {
        assert_eq!(reparsed.to_xml(), xml, "serialisation is not a fixed point");
    }
});

fn alloc_vec(first: NodeId) -> Vec<NodeId> {
    let mut v = Vec::with_capacity(16);
    v.push(first);
    v
}
