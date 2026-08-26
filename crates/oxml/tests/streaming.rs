// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading a document as events, without building a tree.
//!
//! The property that matters is not that the reader works, but that it
//! **agrees with the tree parser** — same scanner, so the same
//! documents accepted, the same rejected, and the same content found.
//! A second XML scanner that drifted would be worse than none.

use oxml::parse;
use oxml::stream::{Event, Reader};

/// Every event a document yields.
fn events(input: &str) -> Result<Vec<Event>, oxml::Error> {
    let mut reader = Reader::new(input)?;
    let mut out = Vec::new();
    while let Some(event) = reader.next_event()? {
        out.push(event);
    }
    Ok(out)
}

#[test]
fn a_document_yields_its_constructs_in_order() {
    let found = events("<a><b>text</b><!--c--><?pi d?></a>").expect("valid");
    let described: Vec<String> = found
        .iter()
        .map(|e| match e {
            Event::StartElement { name, .. } => format!("<{}>", name.local),
            Event::EndElement { name } => format!("</{}>", name.local),
            Event::Text(t) => format!("text {t:?}"),
            Event::Comment(c) => format!("comment {c:?}"),
            Event::ProcessingInstruction { target, .. } => {
                format!("pi {target}")
            }
            _ => "?".to_owned(),
        })
        .collect();
    assert_eq!(
        described,
        [
            "<a>",
            "<b>",
            "text \"text\"",
            "</b>",
            "comment \"c\"",
            "pi pi",
            "</a>",
        ]
    );
}

#[test]
fn a_self_closing_element_yields_both_halves() {
    let found = events("<a><b/></a>").expect("valid");
    assert!(matches!(found[1], Event::StartElement { .. }));
    assert!(matches!(found[2], Event::EndElement { .. }));
    // So a caller counting depth need not treat it specially.
    let depth: i32 = found
        .iter()
        .map(|e| match e {
            Event::StartElement { .. } => 1,
            Event::EndElement { .. } => -1,
            _ => 0,
        })
        .sum();
    assert_eq!(depth, 0, "every start is matched by an end");
}

#[test]
fn namespaces_are_resolved_as_the_tree_parser_resolves_them() {
    let found =
        events(r#"<r xmlns:m="urn:u"><m:a k="1"/></r>"#).expect("valid");
    let Event::StartElement { name, attributes } = &found[1] else {
        panic!("expected a start element, got {:?}", found[1]);
    };
    assert_eq!(name.local, "a");
    assert_eq!(name.namespace.as_deref(), Some("urn:u"));
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].0.local, "k");
    // An unprefixed attribute is in no namespace even here.
    assert_eq!(attributes[0].0.namespace, None);
}

#[test]
fn entities_and_cdata_arrive_expanded_and_merged() {
    let found =
        events("<a>one &amp; <![CDATA[two & three]]> four</a>").expect("valid");
    let text: String = found
        .iter()
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "one & two & three four");
}

/// The whole point: the reader and the tree parser must agree.
#[test]
fn the_reader_and_the_tree_parser_accept_the_same_documents() {
    let cases = [
        "<a/>",
        "<a></a>",
        "<a><b/><c/></a>",
        "<a x='1' y='2'/>",
        r#"<r xmlns="urn:d" xmlns:m="urn:u"><m:a/><b/></r>"#,
        "<a>text &amp; more</a>",
        "<a><![CDATA[<not markup>]]></a>",
        "<!-- before --><a/><!-- after -->",
        "<?xml version='1.0'?><a/>",
        "<?xml version='1.0' encoding='UTF-8'?><a><b>1</b></a>",
        "<a>&#65;&#x42;</a>",
        "<!DOCTYPE a><a/>",
        // A DTD-declared entity is used after the DOCTYPE has been
        // consumed, so the declarations must survive between events.
        r#"<!DOCTYPE a [<!ENTITY x "expanded">]><a>&x;</a>"#,
        r#"<!DOCTYPE a [<!ENTITY x "one">]><a><b>&x;</b><c>&x;</c></a>"#,
        // Rejected by both.
        "<a>",
        "<a></b>",
        "<a x='1' x='2'/>",
        "</a>",
        "<a/><b/>",
        "<a/>trailing",
        "<a/><!DOCTYPE a>",
        "<!DOCTYPE a><!DOCTYPE a><a/>",
        "<a>&undefined;</a>",
        "<a x=1/>",
        "<a><![CDATA[unterminated</a>",
        "<a>]]></a>",
        "<",
        "<!-- no root -->",
        "",
    ];
    for input in cases {
        // Not merely "both failed" -- the same failure. A reader that
        // rejected everything would pass the weaker check.
        let tree = parse(input).map(|_| ()).map_err(|e| e.kind);
        let stream = events(input).map(|_| ()).map_err(|e| e.kind);
        assert_eq!(tree, stream, "{input:?}");
    }
}

/// And on the documents both accept, they must find the same content.
#[test]
fn the_reader_and_the_tree_parser_find_the_same_content() {
    let cases = [
        "<a><b>one</b><c>two</c></a>",
        r#"<r xmlns:m="urn:u"><m:a>x</m:a></r>"#,
        "<a>text &amp; <![CDATA[raw]]> more</a>",
        "<a><b><c><d>deep</d></c></b></a>",
        "<a x='1'><b y='2'/></a>",
    ];
    for input in cases {
        // Element names in document order, from the tree.
        let doc = parse(input).expect("valid");
        let from_tree: Vec<String> = doc
            .descendants()
            .filter_map(|id| doc.element_name(id).map(|n| n.local.clone()))
            .collect();

        let from_stream: Vec<String> = events(input)
            .expect("valid")
            .into_iter()
            .filter_map(|e| match e {
                Event::StartElement { name, .. } => Some(name.local),
                _ => None,
            })
            .collect();
        assert_eq!(from_tree, from_stream, "{input:?}");

        // And the text, which is the other half of what a document is.
        let tree_text = doc.text(doc.root());
        let stream_text: String = events(input)
            .expect("valid")
            .into_iter()
            .filter_map(|e| match e {
                Event::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tree_text, stream_text, "{input:?}");
    }
}

#[test]
fn an_error_reports_the_same_offset_the_tree_parser_reports() {
    for input in ["<a></b>", "<a x='1' x='2'/>", "<a>&undefined;</a>"] {
        let tree = parse(input).expect_err("invalid");
        let stream = events(input).expect_err("invalid");
        assert_eq!(
            tree.offset, stream.offset,
            "{input:?}: offsets differ ({} vs {})",
            tree.offset, stream.offset
        );
    }
}

/// A failed reader stays failed rather than resuming from wherever the
/// error left the cursor.
#[test]
fn a_reader_that_has_failed_does_not_resume() {
    let mut reader = Reader::new("<a></b>").expect("prolog is fine");
    assert!(reader.next_event().is_ok(), "the start tag reads");
    assert!(reader.next_event().is_err(), "the end tag does not");
    assert!(
        matches!(reader.next_event(), Ok(None)),
        "and nothing follows it"
    );
}

#[test]
fn limits_apply_to_the_reader_as_they_do_to_the_parser() {
    let deep = "<a>".repeat(oxml::MAX_DEPTH + 10);
    let mut reader = Reader::new(&deep).expect("prolog is fine");
    let mut error = None;
    while let Some(_event) = match reader.next_event() {
        Ok(e) => e,
        Err(e) => {
            error = Some(e);
            None
        }
    } {}
    let error = error.expect("must refuse to nest without bound");
    assert!(matches!(error.kind, oxml::ErrorKind::DepthLimitExceeded));
}

/// No tree is built, which is the reason to use this at all.
#[test]
fn reading_a_large_document_does_not_hold_it() {
    // A document far larger than the events it yields at any moment.
    use core::fmt::Write as _;

    let mut source = String::from("<catalogue>");
    for i in 0..20_000 {
        let _ = write!(source, "<item id=\"{i}\">value {i}</item>");
    }
    source.push_str("</catalogue>");

    let mut reader = Reader::new(&source).expect("valid");
    let mut items = 0usize;
    while let Some(event) = reader.next_event().expect("valid") {
        if let Event::StartElement { name, .. } = event {
            if name.local == "item" {
                items += 1;
            }
        }
    }
    assert_eq!(items, 20_000);
}

/// The entity-expansion budget is per document, not per event.
///
/// Each event used to build a parser with a full budget, so a bomb
/// spread across fifty text nodes was handed the budget fifty times
/// and the limit never tripped.
#[test]
fn the_entity_budget_is_spent_across_the_whole_document() {
    use core::fmt::Write as _;

    let mut bomb = String::from(r#"<!DOCTYPE a [<!ENTITY e0 "aaaaaaaaaa">"#);
    for i in 1..9 {
        let _ = write!(bomb, r#"<!ENTITY e{i} ""#);
        for _ in 0..10 {
            let _ = write!(bomb, "&e{};", i - 1);
        }
        bomb.push_str(r#"">"#);
    }
    bomb.push_str("]><a>");
    // Split across many elements: no single run is large enough to
    // trip a per-event budget.
    for _ in 0..50 {
        bomb.push_str("<b>&e8;</b>");
    }
    bomb.push_str("</a>");

    let tree = parse(&bomb).expect_err("must refuse to expand");
    let stream = events(&bomb).expect_err("must refuse to expand");
    assert_eq!(tree.kind, oxml::ErrorKind::EntityLimitExceeded);
    assert_eq!(stream.kind, tree.kind, "the reader must refuse it too");
}

/// Entities declared in the internal subset reach the events that use
/// them.
#[test]
fn dtd_declared_entities_expand_in_events() {
    let doc = r#"<!DOCTYPE a [<!ENTITY x "expanded">]><a><b>&x;</b></a>"#;
    let text: String = events(doc)
        .expect("valid")
        .into_iter()
        .filter_map(|e| match e {
            Event::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(text, "expanded");
}

/// The depth limit trips at the same nesting level on both paths.
///
/// The tree parser checks depth as it recurses; the reader checks it
/// against a counter. Off by one between them would mean a document
/// one entry point accepts and the other refuses.
#[test]
fn the_depth_limit_trips_at_the_same_level() {
    for depth in 1..8usize {
        let mut limits = oxml::Limits::default();
        limits.max_depth = depth;
        for nesting in 1..10usize {
            let doc =
                format!("{}{}", "<a>".repeat(nesting), "</a>".repeat(nesting));
            let tree = oxml::parse_with(&doc, limits)
                .map(|_| ())
                .map_err(|e| e.kind);

            let mut reader =
                Reader::with_limits(&doc, limits).expect("prolog is fine");
            let stream = loop {
                match reader.next_event() {
                    Ok(Some(_)) => {}
                    Ok(None) => break Ok(()),
                    Err(e) => break Err(e.kind),
                }
            };
            assert_eq!(
                tree, stream,
                "max_depth={depth}, {nesting} levels deep"
            );
        }
    }
}

/// A self-closing element at the limit is treated alike too.
#[test]
fn a_self_closing_element_at_the_limit_agrees() {
    for depth in 1..5usize {
        let mut limits = oxml::Limits::default();
        limits.max_depth = depth;
        for nesting in 1..7usize {
            let doc = format!(
                "{}<leaf/>{}",
                "<a>".repeat(nesting),
                "</a>".repeat(nesting)
            );
            let tree = oxml::parse_with(&doc, limits)
                .map(|_| ())
                .map_err(|e| e.kind);
            let mut reader =
                Reader::with_limits(&doc, limits).expect("prolog is fine");
            let stream = loop {
                match reader.next_event() {
                    Ok(Some(_)) => {}
                    Ok(None) => break Ok(()),
                    Err(e) => break Err(e.kind),
                }
            };
            assert_eq!(
                tree, stream,
                "max_depth={depth}, {nesting} levels then a self-closing leaf"
            );
        }
    }
}

/// Constructs that live outside the root element.
///
/// The prolog and epilog are a different grammar from element
/// content, and they had no test: comments and processing
/// instructions either side of the root, and a `DOCTYPE` that a
/// comment pushes past the prolog scan.
#[test]
fn the_prolog_and_epilog_yield_their_constructs() {
    let doc = "<?first?><!-- before --><!DOCTYPE a><a/><?last?><!-- after -->";
    let found = events(doc).expect("valid");
    let described: Vec<&str> = found
        .iter()
        .map(|e| match e {
            Event::ProcessingInstruction { .. } => "pi",
            Event::Comment(_) => "comment",
            Event::StartElement { .. } => "start",
            Event::EndElement { .. } => "end",
            _ => "?",
        })
        .collect();
    // The declaration-shaped PI and DOCTYPE are prolog, not events;
    // what a caller sees is the comments, the element, and the PI.
    assert!(described.contains(&"start"), "{described:?}");
    assert!(described.contains(&"comment"), "{described:?}");
    assert!(described.contains(&"pi"), "{described:?}");
    assert!(parse(doc).is_ok(), "and the tree parser agrees");
}

#[test]
fn a_doctype_after_a_comment_is_still_the_prolog() {
    // `skip_prolog` stops at the comment, so the `DOCTYPE` is met
    // again outside the root -- where it is legal exactly once and
    // only before the element.
    for doc in [
        "<!-- c --><!DOCTYPE a><a/>",
        r#"<!-- c --><!DOCTYPE a [<!ENTITY x "y">]><a>&x;</a>"#,
    ] {
        let tree = parse(doc).map(|_| ()).map_err(|e| e.kind);
        let stream = events(doc).map(|_| ()).map_err(|e| e.kind);
        assert_eq!(tree, stream, "{doc:?}");
        assert!(tree.is_ok(), "{doc:?} is well-formed: {tree:?}");
    }
}

#[test]
fn an_unterminated_end_tag_is_refused() {
    for doc in ["<a></a", "<a></a x", "<a></"] {
        let tree = parse(doc).map(|_| ()).map_err(|e| e.kind);
        let stream = events(doc).map(|_| ()).map_err(|e| e.kind);
        assert_eq!(tree, stream, "{doc:?}");
        assert!(tree.is_err(), "{doc:?} must be refused");
    }
}

/// A text run longer than the limit is refused, as it is on a parse.
#[test]
fn the_text_length_limit_applies_to_a_run() {
    let mut limits = oxml::Limits::default();
    limits.max_text_length = Some(16);

    // Split by a CDATA section, so the limit must apply to the whole
    // run rather than to each piece: neither piece exceeds 16.
    let doc = "<a>0123456789<![CDATA[abcdefghij]]></a>";
    let tree = oxml::parse_with(doc, limits)
        .map(|_| ())
        .map_err(|e| e.kind);

    let mut reader = Reader::with_limits(doc, limits).expect("prolog is fine");
    let stream = loop {
        match reader.next_event() {
            Ok(Some(_)) => {}
            Ok(None) => break Ok(()),
            Err(e) => break Err(e.kind),
        }
    };
    assert_eq!(stream, Err(oxml::ErrorKind::TextTooLong));
    assert_eq!(tree, stream, "the limit must mean the same on both");

    // And a run inside the limit is accepted.
    let short = "<a>0123</a>";
    let mut reader =
        Reader::with_limits(short, limits).expect("prolog is fine");
    while reader.next_event().expect("within the limit").is_some() {}
}

/// A malformed tag *at* the depth limit reports the depth, not the tag.
///
/// Found by fuzzing, 980 bytes into a generated document. The tree
/// parser checks depth on entry to an element; the reader scanned the
/// start tag first and reported whatever the scan tripped over, so the
/// two disagreed only when a document was both too deep and malformed
/// at exactly that point. Every balanced document agreed, which is why
/// the hand-written depth tests missed it.
#[test]
fn a_malformed_tag_at_the_depth_limit_reports_the_depth() {
    let mut limits = oxml::Limits::default();
    limits.max_depth = 4;

    // The first four are start tags, where the depth check applies and
    // must win. The last two are not, so they report what they are --
    // included because both entry points must still agree on them.
    for (tail, depth_first) in [
        ("<", true),
        ("<3", true),
        ("<a b", true),
        ("<a", true),
        ("</", false),
        ("<!", false),
    ] {
        let doc = format!("{}{tail}", "<a>".repeat(4));
        let tree = oxml::parse_with(&doc, limits)
            .map(|_| ())
            .map_err(|e| (e.kind, e.offset));
        let mut reader =
            Reader::with_limits(&doc, limits).expect("prolog is fine");
        let stream = loop {
            match reader.next_event() {
                Ok(Some(_)) => {}
                Ok(None) => break Ok(()),
                Err(e) => break Err((e.kind, e.offset)),
            }
        };
        assert_eq!(tree, stream, "{doc:?}");
        if depth_first {
            assert_eq!(
                stream,
                Err((oxml::ErrorKind::DepthLimitExceeded, 12)),
                "{doc:?} is at the limit before it is malformed"
            );
        }
    }
}
