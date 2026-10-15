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

use oxml::{Document, EmptyElement, Indent, SerialiseError, SerialiseOptions};

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

    // `write_xml_to` writes bytes into any `std::io::Write` -- a file,
    // a socket, a `Vec<u8>` -- and is byte-identical to `to_xml_with`
    // with the same options. The first I/O error is returned as is.
    let mut bytes = Vec::new();
    nested
        .write_xml_to(&mut bytes, &SerialiseOptions::default())
        .expect("writing to a Vec cannot fail");
    assert_eq!(bytes, nested.to_xml().into_bytes());

    // The declaration is written only when the content needs XML 1.1:
    // a C0 control character, which 1.0 cannot express at all. The C1
    // range is legal in 1.0 and goes out as a reference without one.
    let plain = oxml::parse("<a>caf\u{85}</a>").expect("well-formed");
    assert_eq!(plain.to_xml(), "<a>caf&#133;</a>");
    let control = oxml::parse("<?xml version=\"1.1\"?><a>&#1;</a>")
        .expect("well-formed in 1.1");
    assert_eq!(control.to_xml(), "<?xml version=\"1.1\"?><a>&#1;</a>");

    // A built document can hold what no version writes: a control
    // character needs 1.1, and 1.1 reads U+0085 in a comment as a
    // line ending, with no reference syntax there to protect it.
    // `try_to_xml` refuses that document; for any other it returns
    // exactly what `to_xml` does.
    let mut built = Document::empty();
    let top = built
        .append_element(built.root(), None, "a")
        .expect("one root element");
    let note = built.append_comment(top, "caf\u{85}").expect("legal");
    assert_eq!(built.try_to_xml(), Ok(built.to_xml()));
    let _ = built.append_text(top, "\u{1}").expect("expressible");
    match built.try_to_xml() {
        Err(SerialiseError::VersionConflict { node, character }) => {
            assert_eq!((node, character), (note, '\u{85}'));
            println!("refused: {}", built.try_to_xml().unwrap_err());
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}
