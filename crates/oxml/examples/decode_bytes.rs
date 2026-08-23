// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Parsing bytes whose encoding you do not know in advance.
//!
//! `parse` takes a `&str`, which means the caller has already decided
//! the encoding. `parse_bytes` reads the document's own declaration
//! and byte-order mark instead, which is what you want for a file or
//! an HTTP body.
//!
//! Run with:
//!
//! ```text
//! cargo run --example decode_bytes
//! ```

use oxml::encoding::{Encoding, decode, is_legal_encoding_name};
use oxml::{Limits, parse_bytes, parse_bytes_with};

fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut out = if big_endian {
        vec![0xFE, 0xFF]
    } else {
        vec![0xFF, 0xFE]
    };
    for unit in text.encode_utf16() {
        out.extend_from_slice(&if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        });
    }
    out
}

fn main() {
    println!("== the same document in four encodings ==");

    let plain =
        b"<?xml version=\"1.0\"?><greeting>Hi \xC3\xA9</greeting>".to_vec();
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(&plain);
    let mut latin1 =
        b"<?xml version='1.0' encoding='ISO-8859-1'?><greeting>Hi ".to_vec();
    latin1.push(0xE9);
    latin1.extend_from_slice(b"</greeting>");

    for (label, bytes) in [
        ("UTF-8", plain.clone()),
        ("UTF-8 with a BOM", with_bom),
        ("UTF-16BE", utf16("<greeting>Hi \u{e9}</greeting>", true)),
        ("UTF-16LE", utf16("<greeting>Hi \u{e9}</greeting>", false)),
        ("ISO-8859-1", latin1),
    ] {
        match parse_bytes(&bytes) {
            Ok(doc) => {
                let root = doc.root_element().expect("a root element");
                println!(
                    "  {label:<17} {} bytes -> {:?}",
                    bytes.len(),
                    doc.text(root)
                );
            }
            Err(e) => println!("  {label:<17} failed: {e}"),
        }
    }

    // Limits apply to the byte entry point too; the encoding layer
    // runs before any of them, since you cannot bound a document you
    // cannot read.
    let doc = parse_bytes_with(&plain, Limits::strict());
    println!(
        "\nunder strict limits: {}",
        if doc.is_ok() { "ok" } else { "refused" }
    );

    println!("\n== two different kinds of encoding failure ==");
    // A name that production 81 forbids is a malformed *document*:
    // every conforming parser must reject it. A legal name naming an
    // encoding this crate lacks is a different matter -- the document
    // may be perfectly well-formed, and the caller can decode it with
    // a crate that knows the encoding and then call `parse`.
    for src in [
        "<?xml version='1.0' encoding='UTF~8'?><a/>",
        "<?xml version='1.0' encoding='Shift_JIS'?><a/>",
    ] {
        let name = src.split('\'').nth(3).unwrap_or("?");
        println!(
            "  {name:<10} legal name? {:<5} known? {:<5} -> {}",
            is_legal_encoding_name(name),
            Encoding::from_name(name).is_some(),
            match parse_bytes(src.as_bytes()) {
                Ok(_) => "parsed".to_string(),
                Err(e) => e.kind.to_string(),
            }
        );
    }

    println!("\n== decoding without parsing ==");
    // `decode` is the encoding layer on its own, for a caller that
    // wants the text. UTF-8 is borrowed rather than copied, so the
    // common case costs nothing.
    let borrowed = decode(&plain).expect("valid UTF-8");
    println!(
        "  UTF-8 input is borrowed: {}",
        matches!(borrowed, std::borrow::Cow::Borrowed(_))
    );
    let utf16_bytes = utf16("<a/>", true);
    let transcoded = decode(&utf16_bytes).expect("valid UTF-16");
    println!(
        "  UTF-16 input is copied:  {}",
        matches!(transcoded, std::borrow::Cow::Owned(_))
    );
}
