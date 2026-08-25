// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Walk a parsed document by hand.
//!
//! Every node is a `NodeId` -- a plain index into the document's
//! arena, `Copy` and 4 bytes wide. Nothing is borrowed from the tree
//! while you hold one, so you can collect ids, store them, and come
//! back to them later without fighting the borrow checker.
//!
//! Run with:
//!
//! ```text
//! cargo run --example walk_the_tree
//! ```

use oxml::{ExpandedName, NodeKind, parse};

const DOC: &str = r#"<?xml version="1.0"?>
<!-- a short report -->
<?render mode="draft"?>
<report xmlns:m="urn:example:meta">
  <section title="Findings">
    <p>The <em>first</em> finding.</p>
    <p>The second.</p>
  </section>
  <m:footer>Confidential</m:footer>
</report>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = parse(DOC)?;

    // `root()` is the document node, above the root *element*. The
    // comment and processing instruction before `<report>` are its
    // children, which is why the two are not the same node.
    let root = doc.root();
    let element = doc.root_element().expect("a root element");
    println!("document node : {}", root.index());
    println!("root element  : {}", element.index());
    println!("nodes in total: {}", doc.len());
    println!("empty?        : {}", doc.is_empty());

    println!("\n== children of the document node ==");
    for &child in doc.children(root) {
        // `kind` is how you tell the six node types apart.
        let what = match doc.kind(child) {
            Some(NodeKind::Element { .. }) => "element",
            Some(NodeKind::Comment(text)) => {
                println!("  comment: {}", text.trim());
                continue;
            }
            Some(NodeKind::ProcessingInstruction { target, .. }) => {
                println!("  processing instruction: {target}");
                continue;
            }
            Some(NodeKind::Text(_)) => "text",
            Some(NodeKind::Attr(_)) => "attribute",
            // Neither attributes nor namespace declarations are
            // children, so neither can actually appear here — the arms
            // exist because `kind` can return them from elsewhere.
            Some(NodeKind::Namespace { .. }) => "namespace",
            Some(NodeKind::Root) => "root",
            None => "out of range",
        };
        println!("  {what}");
    }

    println!("\n== the whole tree, indented by depth ==");
    // `descendants` visits every node in document order.
    for id in doc.descendants() {
        if !doc.is_element(id) {
            continue;
        }
        // Depth is not stored; walk up to find it. `parent` returns
        // `None` for the document node, which terminates the loop.
        let mut depth = 0;
        let mut cursor = id;
        while let Some(p) = doc.parent(cursor) {
            depth += 1;
            cursor = p;
        }
        let name = doc.element_name(id).expect("an element has a name");
        let indent = "  ".repeat(depth);
        match &name.namespace {
            Some(uri) => println!("{indent}{} (in {uri})", name.local),
            None => println!("{indent}{}", name.local),
        }
    }

    println!("\n== comparing names ==");
    // Names compare by namespace URI and local part, never by prefix:
    // `<m:footer>` and `<meta:footer>` bound to the same URI are the
    // same name, and a document that renames its prefixes still
    // matches.
    let footer = ExpandedName::qualified("urn:example:meta", "footer");
    let plain = ExpandedName::local("footer");
    for id in doc.descendants() {
        if doc.element_name(id) == Some(&footer) {
            println!("  matched the namespaced footer: {}", doc.text(id));
        }
        if doc.element_name(id) == Some(&plain) {
            println!("  matched an un-namespaced footer");
        }
    }

    println!("\n== text ==");
    // `text` concatenates every descendant text node, so markup inside
    // a paragraph disappears and the sentence survives intact.
    let section = doc
        .descendants()
        .find(|&id| doc.element_name(id).is_some_and(|n| n.local == "p"))
        .expect("a paragraph");
    println!("  {:?}", doc.text(section));

    // An id past the end of the arena is `None` rather than a panic.
    let past_end = doc.descendants().last().expect("a last node");
    println!("\nlast node index: {}", past_end.index());
    Ok(())
}
