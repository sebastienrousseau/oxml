// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Conditional sections in an external subset.
//!
//! `<![INCLUDE[ … ]]>` and `<![IGNORE[ … ]]>` are legal only in the
//! external subset, and the keyword is usually written as a parameter
//! entity so one document can test both branches:
//!
//! ```text
//! <!ENTITY % MAYBE "IGNORE">
//! <![%MAYBE;[ … ]]>
//! ```
//!
//! Two things made this worth its own file. A section whose keyword is
//! neither `INCLUDE` nor `IGNORE` was accepted in silence, because a
//! declaration containing a parameter entity took a path that reports
//! anything unparseable as merely *incomplete*. And a parameter entity
//! is replacement **text**, not a token, so it may carry the opening
//! bracket with it -- `<!ENTITY % e "INCLUDE[">` is in the W3C suite.

use oxml::{Limits, parse, parse_with_external};

/// Parse `doc` with `dtd` available as `d.dtd`.
fn with_dtd(doc: &str, dtd: &str) -> Result<(), oxml::ErrorKind> {
    let source: &[(&str, &str)] = &[("d.dtd", dtd)];
    parse_with_external(doc, Limits::default(), &source)
        .map(|_| ())
        .map_err(|e| e.kind)
}

const DOC: &str = r#"<!DOCTYPE root SYSTEM "d.dtd">
<root/>"#;

#[test]
fn include_and_ignore_are_the_only_keywords() {
    for keyword in ["INCLUDE", "IGNORE"] {
        let dtd = format!(
            "<!ELEMENT root EMPTY>\n<![{keyword}[ <!ELEMENT a EMPTY> ]]>"
        );
        assert!(with_dtd(DOC, &dtd).is_ok(), "{keyword} is a keyword");
    }
    for keyword in ["CDATA", "INCLUDED", "", "include"] {
        let dtd = format!(
            "<!ELEMENT root EMPTY>\n<![{keyword}[ <!ELEMENT a EMPTY> ]]>"
        );
        assert!(
            with_dtd(DOC, &dtd).is_err(),
            "{keyword:?} is not a conditional-section keyword"
        );
    }
}

/// The keyword arriving through a parameter entity is the normal way
/// to write one, and the invalid ones must still be caught.
#[test]
fn a_keyword_from_a_parameter_entity_is_still_checked() {
    for (value, ok) in [
        ("INCLUDE", true),
        ("IGNORE", true),
        ("CDATA", false),
        ("", false),
    ] {
        let dtd = format!(
            "<!ENTITY % MAYBE \"{value}\">\n<![%MAYBE;[ <!ELEMENT a EMPTY> ]]>"
        );
        assert_eq!(
            with_dtd(DOC, &dtd).is_ok(),
            ok,
            "keyword {value:?} from a parameter entity"
        );
    }
}

/// The internal subset's declaration wins, which is how the suite
/// turns one external subset into two tests.
#[test]
fn the_internal_subset_overrides_the_keyword() {
    let dtd =
        "<!ENTITY % MAYBE \"IGNORE\">\n<![%MAYBE;[ <!ENTITY root EMTPY> ]]>";

    // Overridden to something that is not a keyword: refused.
    for bad in ["CDATA", ""] {
        let doc = format!(
            "<!DOCTYPE root SYSTEM \"d.dtd\" [\n<!ENTITY % MAYBE \"{bad}\">\n]>\n<root/>"
        );
        assert!(
            with_dtd(&doc, dtd).is_err(),
            "{bad:?} from the internal subset must be refused"
        );
    }

    // Overridden to a real keyword: accepted.
    let doc = "<!DOCTYPE root SYSTEM \"d.dtd\" [\n<!ENTITY % MAYBE \"IGNORE\">\n]>\n<root/>";
    assert!(with_dtd(doc, dtd).is_ok());
}

/// A parameter entity may carry the opening bracket.
///
/// `<!ENTITY % e "INCLUDE[">` used as `<![ %e; … ]]>` is legal, and
/// requiring a literal `[` after the reference rejected it.
#[test]
fn a_parameter_entity_may_supply_the_bracket() {
    let dtd = "<!ENTITY % e \"INCLUDE[\">\n<!ELEMENT doc (#PCDATA)>\n<![ %e; <!ATTLIST doc a1 CDATA \"v1\"> ]]>";
    let doc = "<!DOCTYPE doc SYSTEM \"d.dtd\">\n<doc></doc>";
    assert!(
        with_dtd(doc, dtd).is_ok(),
        "an entity may carry the bracket with the keyword"
    );

    // And the keyword is still checked when it arrives that way.
    let bad = "<!ENTITY % e \"CDATA[\">\n<!ELEMENT doc (#PCDATA)>\n<![ %e; <!ELEMENT a EMPTY> ]]>";
    assert!(with_dtd(doc, bad).is_err(), "{bad:?} is not a keyword");
}

/// Without the external subset there is nothing to judge.
///
/// oxml performs no I/O, so a document naming a DTD the caller did not
/// supply is accepted: the declarations that would have decided it
/// were never read, and refusing would reject valid documents.
#[test]
fn an_unavailable_subset_is_not_an_error() {
    let doc = "<!DOCTYPE root SYSTEM \"d.dtd\" [\n<!ENTITY % MAYBE \"CDATA\">\n]>\n<root/>";
    assert!(parse(doc).is_ok(), "nothing was read, so nothing is wrong");
}

/// A conditional section is still illegal in the internal subset.
#[test]
fn conditional_sections_stay_out_of_the_internal_subset() {
    let doc = "<!DOCTYPE root [\n<!ELEMENT root EMPTY>\n<![INCLUDE[ <!ELEMENT a EMPTY> ]]>\n]>\n<root/>";
    assert!(parse(doc).is_err(), "only the external subset may have one");
}
