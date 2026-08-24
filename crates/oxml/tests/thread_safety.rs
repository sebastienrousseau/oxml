// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The README says a parsed document can be queried from any number of
//! threads. That is a property of the types, and a field added in a
//! later version could quietly take it away, so it is asserted here
//! rather than believed.

use oxml::{
    Attribute, Document, Error, ErrorKind, ExpandedName, Limits, NodeId,
    NodeKind, parse,
};

const fn assert_send_sync<T: Send + Sync>() {}
const fn assert_copy<T: Copy>() {}

#[test]
fn the_public_types_cross_thread_boundaries() {
    assert_send_sync::<Document>();
    assert_send_sync::<NodeId>();
    assert_send_sync::<NodeKind>();
    assert_send_sync::<Attribute>();
    assert_send_sync::<ExpandedName>();
    assert_send_sync::<Error>();
    assert_send_sync::<ErrorKind>();
    assert_send_sync::<Limits>();
    #[cfg(feature = "xpath")]
    assert_send_sync::<oxml::XPath>();
    #[cfg(feature = "xpath")]
    assert_send_sync::<oxml::XPathError>();
}

#[test]
fn a_node_id_is_a_cheap_copyable_index() {
    // The README's argument for ids over references is that they are
    // `Copy` and small enough to pass around freely.
    assert_copy::<NodeId>();
    // Pointer-sized: it is an index into the document's arena. The
    // README says "pointer-sized" rather than a number of bytes
    // because that is what it is, on whatever target you build for.
    assert_eq!(
        core::mem::size_of::<NodeId>(),
        core::mem::size_of::<usize>()
    );
}

#[test]
fn one_document_can_be_queried_from_many_threads() {
    // The static assertions above prove the types allow it. This
    // proves it actually works, which is not quite the same claim.
    let doc =
        parse("<r><a id='1'>x</a><a id='2'>y</a></r>").expect("well-formed");
    let root = doc.root_element().expect("a root element");

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let ids: Vec<_> = doc
                        .children(root)
                        .iter()
                        .filter_map(|&c| doc.attribute(c, "id"))
                        .collect();
                    ids.join(",")
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().expect("no panic"), "1,2");
        }
    });
}

#[cfg(feature = "xpath")]
#[test]
fn one_compiled_expression_can_be_evaluated_from_many_threads() {
    // Compile-once-evaluate-many across a thread pool is the reason
    // `XPath` being `Sync` matters.
    use oxml::XPath;

    let query = XPath::compile("count(//a)").expect("valid");
    let doc = parse("<r><a/><a/><a/></r>").expect("well-formed");

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    assert_eq!(query.evaluate(&doc).to_str(&doc), "3");
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("no panic");
        }
    });
}
