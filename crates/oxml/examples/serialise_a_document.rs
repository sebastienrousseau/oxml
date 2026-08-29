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
}
