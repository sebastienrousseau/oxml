// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Qualified names, declared IDs, and the functions that read them.
//!
//! Three of `XPath`'s functions ask questions the tree cannot answer
//! from the expanded name alone:
//!
//! - `name()` wants the prefix, which namespace resolution discards.
//! - `id()` wants to know which attributes are ID-typed, which only a
//!   DTD declares -- an attribute is not an ID because it is spelled
//!   `id`.
//! - `lang()` wants `xml:lang`, which is inherited from an ancestor
//!   rather than carried by the node that needs it.
//!
//! [`Document::prefix`] and [`Document::element_by_id`] expose the
//! first two directly.
//!
//! Run with:
//!
//! ```text
//! cargo run --example names_and_ids
//! ```

use oxml::{NodeKind, XPath, parse};

/// Two prefixes bound to one namespace, an ID-typed attribute declared
/// in the internal subset, and a language declared once on an ancestor.
const DOC: &str = r#"<?xml version="1.0"?>
<!DOCTYPE catalogue [
  <!ELEMENT catalogue ANY>
  <!ELEMENT item ANY>
  <!ATTLIST item sku ID #REQUIRED>
]>
<catalogue xmlns:a="urn:example:ns" xmlns:b="urn:example:ns"
           xml:lang="en-GB">
  <item sku="AX-1" id="not-an-id">Widget</item>
  <item sku="AX-2">Sprocket</item>
  <a:note>first prefix</a:note>
  <b:note>second prefix</b:note>
</catalogue>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = parse(DOC)?;

    // `name()` keeps the prefix; `local-name()` does not. Both `note`
    // elements are the *same* expanded name, so only the prefix tells
    // them apart.
    println!("== qualified names ==");
    for (expr, label) in [
        ("name(//*[local-name()='note'][1])", "name()      "),
        ("local-name(//*[local-name()='note'][1])", "local-name()"),
        ("name(//*[local-name()='note'][2])", "name()      "),
        (
            "namespace-uri(//*[local-name()='note'][2])",
            "namespace-uri()",
        ),
    ] {
        let value = XPath::compile(expr)?.evaluate(&doc).to_str(&doc);
        println!("  {label} -> {value:?}");
    }

    // The same answer without XPath: every element's name id resolves
    // to an expanded name and, separately, to the prefix it was
    // written with.
    println!("\n== prefixes from the tree ==");
    for id in doc.descendants() {
        let Some(NodeKind::Element { name, .. }) = doc.kind(id) else {
            continue;
        };
        let (name, prefix) = (*name, doc.prefix(*name));
        let expanded = doc.name(name).expect("interned");
        if expanded.namespace.is_some() {
            println!(
                "  {:?} in {:?} written with prefix {:?}",
                expanded.local,
                expanded.namespace.as_deref().unwrap_or(""),
                prefix.unwrap_or("(none)")
            );
        }
    }

    // Namespace declarations are nodes too, reachable from the
    // `namespace::` axis. `namespace_nodes` gives the ones written on
    // an element; the axis adds everything inherited from ancestors.
    println!("\n== namespace declarations ==");
    let catalogue = doc.root_element().expect("a root element");
    for &ns in doc.namespace_nodes(catalogue) {
        if let Some(NodeKind::Namespace { prefix, uri }) = doc.kind(ns) {
            let shown = if prefix.is_empty() {
                "(default)"
            } else {
                prefix
            };
            println!("  declared on <catalogue>: {shown} -> {uri}");
        }
    }
    // `xml` is bound by specification, so it is in scope without being
    // written anywhere -- including on elements that declare nothing.
    let in_scope = XPath::compile("count(//item[1]/namespace::*)")?
        .evaluate(&doc)
        .to_str(&doc);
    println!("  in scope for <item>: {in_scope} (a, b and the implicit xml)");
    let xml_uri = XPath::compile("string(//item[1]/namespace::xml)")?
        .evaluate(&doc)
        .to_str(&doc);
    println!("  namespace::xml     : {xml_uri}");

    // `sku` is declared ID, so `id()` finds it. `id` is not declared
    // anything, so it is an ordinary attribute however it is spelled.
    println!("\n== declared IDs ==");
    for value in ["AX-1", "AX-2", "not-an-id"] {
        let found = doc.element_by_id(value);
        let text = found.map(|n| doc.text(n));
        println!("  element_by_id({value:?}) -> {text:?}");
    }
    let n = XPath::compile("count(id('AX-1 AX-2'))")?
        .evaluate(&doc)
        .to_str(&doc);
    println!("  id() takes a list: count(id('AX-1 AX-2')) = {n}");

    // `xml:lang` is declared once on the root and inherited by every
    // item below it. A subtag matches the bare tag, so `en` matches
    // `en-GB`.
    println!("\n== inherited language ==");
    for expr in [
        "count(//item[lang('en')])",
        "count(//item[lang('en-GB')])",
        "count(//item[lang('fr')])",
    ] {
        let value = XPath::compile(expr)?.evaluate(&doc).to_str(&doc);
        println!("  {expr} = {value}");
    }

    // The string functions that read around a separator.
    println!("\n== splitting on a separator ==");
    let sku = XPath::compile("string(//item/@sku)")?
        .evaluate(&doc)
        .to_str(&doc);
    for expr in [
        "substring-before(//item/@sku, '-')",
        "substring-after(//item/@sku, '-')",
        "translate(//item/@sku, '-', '/')",
    ] {
        let value = XPath::compile(expr)?.evaluate(&doc).to_str(&doc);
        println!("  {expr}\n    on {sku:?} -> {value:?}");
    }

    // A name the library does not implement fails to compile rather
    // than evaluating to nothing, so a typo cannot read as "no match".
    println!("\n== unknown functions ==");
    match XPath::compile("substring-bfore('a-b','-')") {
        Ok(_) => println!("  compiled, which it should not have"),
        Err(e) => println!("  substring-bfore(...) -> {e}"),
    }

    Ok(())
}
