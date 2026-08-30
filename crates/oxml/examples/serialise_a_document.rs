// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Writing a document back out as XML.
//!
//! Run with:
//!
//! ```text
//! cargo run --example serialise_a_document
//! ```
//!
//! The point worth understanding is that this is not a byte-exact
//! round trip. The first pass through changes some things — an entity
//! becomes the character it stood for, a CDATA section becomes plain
//! text — and every pass after it changes nothing. That fixed point is
//! the property to rely on.

use std::fmt::Write as _;

use oxml::{EmptyElement, Indent, SerialiseOptions};

fn main() {
    let source = "<?xml version=\"1.0\"?>
<catalogue>
  <book lang='en'><![CDATA[Dune & Children]]></book>
  <!-- prices exclude shipping -->
</catalogue>";

    let doc = oxml::parse(source).expect("the document is well-formed");

    // `to_xml` is the short way.
    let once = doc.to_xml();
    println!("once:\n{once}\n");

    // Serialising again changes nothing, however different the first
    // pass looked from the source: single quotes became double, the
    // CDATA wrapper is gone, and `&` is now written as an entity.
    let twice = oxml::parse(&once).expect("output parses").to_xml();
    assert_eq!(once, twice, "serialisation should be a fixed point");
    println!("serialising again changed nothing\n");

    // `write_xml` writes into anything that implements `fmt::Write`,
    // for callers who would rather not build a `String` first.
    let mut buffer = String::new();
    doc.write_xml(&mut buffer)
        .expect("writing to a String cannot fail");
    assert_eq!(buffer, once);

    // And it composes with anything else that writes.
    let mut report = String::new();
    let _ =
        writeln!(report, "{} nodes, {} bytes of XML", doc.len(), once.len());
    print!("{report}");

    // Formatted output, for humans. Indentation is only inserted
    // between children of elements whose children are all elements --
    // an element with any text child is written exactly as the
    // compact form writes it, because whitespace next to character
    // data becomes part of that data. Safe by construction rather
    // than by a warning.
    let nested = oxml::parse("<config><servers><host/><host/></servers><motd>hi there</motd></config>")
        .expect("well-formed");
    let pretty = nested.to_xml_with(SerialiseOptions {
        indent: Some(Indent::Spaces(2)),
        ..SerialiseOptions::default()
    });
    println!(
        "pretty-printed:
{pretty}
"
    );
    assert!(
        pretty.contains(
            "
  <servers>"
        ),
        "{pretty}"
    );
    assert!(
        pretty.contains("<motd>hi there</motd>"),
        "text content untouched: {pretty}"
    );

    // The default options are exactly `to_xml`, and only the default
    // carries the fixed-point guarantee -- a pretty-printed document
    // reparses with the inserted whitespace as text nodes.
    assert_eq!(
        nested.to_xml_with(SerialiseOptions::default()),
        nested.to_xml()
    );

    // `write_xml_with` is the writer-flavoured form, and the
    // empty-element spelling applies everywhere, mixed content
    // included.
    let mut spaced = String::new();
    nested
        .write_xml_with(
            &mut spaced,
            SerialiseOptions {
                empty_elements: EmptyElement::SelfClosingSpaced,
                ..SerialiseOptions::default()
            },
        )
        .expect("writing to a String cannot fail");
    assert!(spaced.contains("<host />"), "{spaced}");
}
