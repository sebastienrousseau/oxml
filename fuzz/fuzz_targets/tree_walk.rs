#![no_main]
//! Structural invariants of any successfully parsed tree.
//!
//! `parse` fuzzes for panics; this asserts the tree is *coherent*. A
//! document that parses but whose parent and child links disagree would
//! pass a panic-only target while breaking every consumer.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    let Ok(doc) = oxml::parse(text) else {
        return;
    };

    let root = doc.root();
    assert_eq!(doc.parent(root), None, "the root must have no parent");

    for id in doc.descendants() {
        // Every child names this node as its parent.
        for child in doc.children(id) {
            assert_eq!(
                doc.parent(*child),
                Some(id),
                "child/parent links disagree"
            );
        }

        // Attributes are reachable and know their owner, but are never
        // children — that distinction is the whole attribute axis.
        for attr in doc.attribute_nodes(id) {
            assert_eq!(doc.parent(*attr), Some(id), "attribute owner");
            assert!(
                !doc.children(id).contains(attr),
                "an attribute appeared among the children"
            );
        }

        // Walking up terminates, and does so at the root.
        let mut hops = 0usize;
        let mut cur = Some(id);
        while let Some(n) = cur {
            cur = doc.parent(n);
            hops += 1;
            assert!(hops <= doc.len(), "parent chain does not terminate");
        }
    }

    // Adjacent text is coalesced during parsing, so no element may have
    // two text children in a row.
    for id in doc.descendants() {
        let kids = doc.children(id);
        for pair in kids.windows(2) {
            let a = matches!(doc.kind(pair[0]), Some(oxml::NodeKind::Text(_)));
            let b = matches!(doc.kind(pair[1]), Some(oxml::NodeKind::Text(_)));
            assert!(!(a && b), "adjacent text nodes were not merged");
        }
    }
});
