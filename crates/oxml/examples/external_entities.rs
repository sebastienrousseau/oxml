// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! External entities and subsets, supplied by you rather than fetched.
//!
//! oxml performs no I/O. A document referencing an external entity
//! names a *location*, and resolving it is the caller's decision — you
//! have the permission model, the user, and the context. That is why
//! XXE is structurally impossible here rather than switched off: there
//! is no file or socket code to disable.
//!
//! This shows both halves. Without a source, an external reference
//! expands to nothing and nothing is fetched. With one, the same parse
//! also checks the rules only the external content can settle.
//!
//! Run with:
//!
//! ```text
//! cargo run --example external_entities
//! ```

use oxml::external::ExternalSource;
use oxml::{Limits, parse, parse_with_external};

/// The classic XXE payload. Nothing here can read the file, because
/// nothing in the crate opens one.
const XXE: &str = r#"<!DOCTYPE d [
  <!ENTITY secret SYSTEM "file:///etc/passwd">
]><d>&secret;</d>"#;

/// A document split across an external entity and an external subset.
const SPLIT: &str = r#"<!DOCTYPE letter SYSTEM "letter.dtd">
<letter><body>&salutation; and regards</body></letter>"#;

/// A source that answers only for identifiers on an allow-list, which
/// is the shape a real caller wants: resolution is a policy decision,
/// so it belongs in code you can audit.
struct AllowList<'a> {
    allowed: &'a [(&'a str, &'a str)],
}

impl ExternalSource for AllowList<'_> {
    fn fetch(&self, system_id: &str, _public: Option<&str>) -> Option<&str> {
        let hit = self
            .allowed
            .iter()
            .find(|(id, _)| *id == system_id)
            .map(|(_, content)| *content);
        println!(
            "    fetch({system_id:?}) -> {}",
            match hit {
                Some(_) => "supplied",
                None => "refused",
            }
        );
        hit
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Without a source: the reference expands to nothing. No error,
    // no fetch, no file read.
    println!("== no source supplied ==");
    let doc = parse(XXE)?;
    println!("  XXE payload parsed, body = {:?}", doc.text(doc.root()));
    println!("  (the entity expanded to nothing; no file was opened)");

    // The simplest source: a slice of (system identifier, content).
    println!("\n== a slice as a source ==");
    let parts: &[(&str, &str)] = &[("greeting.ent", "hello")];
    println!(
        "  fetch(\"greeting.ent\") = {:?}",
        parts.fetch("greeting.ent", None)
    );
    let doc = parse_with_external(
        r#"<!DOCTYPE d [<!ENTITY g SYSTEM "greeting.ent">]><d>&g;</d>"#,
        Limits::default(),
        &parts,
    )?;
    println!("  parsed body = {:?}", doc.text(doc.root()));

    // An external *subset*: the DTD itself lives outside the document,
    // and its declarations only exist once you supply it.
    println!("\n== an external subset ==");
    let source = AllowList {
        allowed: &[("letter.dtd", r#"<!ENTITY salutation "Dear reader">"#)],
    };
    let doc = parse_with_external(SPLIT, Limits::default(), &source)?;
    println!("  body = {:?}", doc.text(doc.root()));

    // The same document with the subset refused: the declaration is
    // never seen, so the entity expands to nothing rather than failing.
    println!("\n== the same document, subset refused ==");
    let empty = AllowList { allowed: &[] };
    let doc = parse_with_external(SPLIT, Limits::default(), &empty)?;
    println!("  body = {:?}", doc.text(doc.root()));
    println!("  (an unavailable identifier is not an error — supply the");
    println!("   parts you have and leave the rest)");

    Ok(())
}
