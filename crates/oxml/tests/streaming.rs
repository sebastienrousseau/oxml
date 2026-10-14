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
            Event::EndElement { name, .. } => format!("</{}>", name.local),
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
    let Event::StartElement {
        name, attributes, ..
    } = &found[1]
    else {
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
        if let Event::StartElement { name, .. } = event
            && name.local == "item"
        {
            items += 1;
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

/// Zero-copy text and attribute borrowing directly from the stream.
#[test]
fn streaming_text_and_attribute_borrowing() {
    use std::borrow::Cow;

    let xml = r#"<catalog id="c1"><book lang="en">The Rust Book &amp; More</book></catalog>"#;
    let mut reader = Reader::new(xml).expect("valid xml");

    // StartElement catalog
    let ev1 = reader.next_borrowed().expect("ok").expect("some");
    if let oxml::stream::BorrowedEvent::StartElement {
        name, attributes, ..
    } = ev1
    {
        assert_eq!(name.local, "catalog");
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[0].0.local, "id");
        assert!(
            matches!(&attributes[0].1, Cow::Borrowed(val) if *val == "c1"),
            "attribute should be borrowed slice"
        );
    } else {
        panic!("expected StartElement");
    }

    // StartElement book
    let ev2 = reader.next_borrowed().expect("ok").expect("some");
    if let oxml::stream::BorrowedEvent::StartElement {
        name, attributes, ..
    } = ev2
    {
        assert_eq!(name.local, "book");
        assert_eq!(attributes.len(), 1);
        assert!(
            matches!(&attributes[0].1, Cow::Borrowed(val) if *val == "en"),
            "attribute should be borrowed slice"
        );
    } else {
        panic!("expected StartElement");
    }

    // Text: contains &amp; entity, so rewritten to Cow::Owned
    let ev3 = reader.next_borrowed().expect("ok").expect("some");
    if let oxml::stream::BorrowedEvent::Text(cow) = ev3 {
        assert_eq!(cow.as_ref(), "The Rust Book & More");
        assert!(
            matches!(cow, Cow::Owned(_)),
            "expanded entity requires owned string"
        );
    } else {
        panic!("expected Text");
    }

    // Contiguous plain text: strictly borrowed zero-copy
    let plain_xml = "<root><item>Purely Contiguous Text</item></root>";
    let mut plain_reader = Reader::new(plain_xml).expect("valid xml");
    let _ = plain_reader.next_borrowed().expect("ok"); // <root>
    let _ = plain_reader.next_borrowed().expect("ok"); // <item>
    let text_ev = plain_reader.next_borrowed().expect("ok").expect("text");
    if let oxml::stream::BorrowedEvent::Text(cow) = text_ev {
        assert_eq!(cow.as_ref(), "Purely Contiguous Text");
        assert!(
            matches!(cow, Cow::Borrowed(_)),
            "contiguous text MUST borrow zero-copy from input"
        );
    } else {
        panic!("expected Text");
    }
}

/// The node limit trips at the same offset on both paths.
///
/// The tree parser checks the arena before each child; the reader
/// builds no arena, so it counts what the arena would hold -- the
/// document node, elements, their attribute and namespace nodes, text
/// runs, comments and processing instructions -- and checks the count
/// at the same point. `Limits::strict()` bounds nodes, so without this
/// the reader accepted a document the tree parser refused.
#[test]
fn the_node_limit_trips_at_the_same_offset() {
    type Outcome = Result<(), (oxml::ErrorKind, usize)>;
    fn outcome(doc: &str, limits: oxml::Limits) -> (Outcome, Outcome) {
        let tree = oxml::parse_with(doc, limits)
            .map(|_| ())
            .map_err(|e| (e.kind, e.offset));
        let mut reader =
            Reader::with_limits(doc, limits).expect("prolog is fine");
        let stream = loop {
            match reader.next_event() {
                Ok(Some(_)) => {}
                Ok(None) => break Ok(()),
                Err(e) => break Err((e.kind, e.offset)),
            }
        };
        (tree, stream)
    }

    let wide = format!("<r>{}</r>", "<e/>".repeat(100_001));
    let (tree, stream) = outcome(&wide, oxml::Limits::strict());
    assert!(
        matches!(tree, Err((oxml::ErrorKind::TooManyNodes, _))),
        "{tree:?}"
    );
    assert_eq!(tree, stream, "100,001 elements under strict()");

    // Every kind of node, at every small limit, so an off-by-one in
    // what is counted or when shows up as a disagreement.
    let docs = [
        "<r><e/><e/><e/><e/></r>",
        "<r>text<e/>more<e/>tail</r>",
        "<r><!--c--><e/><?p d?><e/></r>",
        "<r a=\"1\" b=\"2\"><e c=\"3\"/><e/></r>",
        "<r xmlns=\"u\" xmlns:p=\"v\"><p:e/><e xmlns:q=\"w\"/></r>",
        "<r><e>one</e><e>two</e><e><![CDATA[three]]></e></r>",
        "<!--before--><?pi?><r><e/></r><!--after-->",
        "<r/>",
    ];
    for doc in docs {
        for max in 1..16usize {
            let mut limits = oxml::Limits::default();
            limits.max_nodes = Some(max);
            let (tree, stream) = outcome(doc, limits);
            assert_eq!(tree, stream, "{doc:?} with max_nodes={max}");
        }
    }
}

/// A tokenized attribute -- one the DTD declares with a type other
/// than `CDATA` -- is collapsed in events as it is in the tree.
#[test]
fn a_tokenized_attribute_collapses_as_it_does_in_the_tree() {
    let doc = "<!DOCTYPE a [<!ATTLIST a x NMTOKEN #IMPLIED>]>\
               <a x=\"  p   q  \" y=\"  r \"/>";
    let tree = parse(doc).expect("valid");
    let root = tree.root_element().expect("root");
    let found = events(doc).expect("valid");
    let Event::StartElement { attributes, .. } = &found[0] else {
        panic!("expected a start element, got {:?}", found[0]);
    };
    let value = |local: &str| {
        attributes
            .iter()
            .find(|(n, _)| n.local == local)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(value("x"), Some("p q"), "declared NMTOKEN, so collapsed");
    assert_eq!(value("x"), tree.attribute(root, "x"));
    assert_eq!(value("y"), Some("  r "), "undeclared, so CDATA");
    assert_eq!(value("y"), tree.attribute(root, "y"));
}

/// A start event carries what a writer needs to put the tag back:
/// the prefix, the declarations, the attributes.
///
/// The writer here is deliberately naive -- it writes what the
/// events say and nothing more -- and the written document must read
/// back as the same events.
fn qname(prefix: Option<&str>, local: &str) -> String {
    prefix.map_or_else(|| local.to_owned(), |p| format!("{p}:{local}"))
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// A prefix bound to `uri` in the scopes open, innermost first.
fn prefix_for(scopes: &[Vec<(Option<String>, String)>], uri: &str) -> String {
    scopes
        .iter()
        .rev()
        .flat_map(|scope| scope.iter())
        .find_map(|(p, u)| (u == uri).then(|| p.clone()).flatten())
        .expect("an attribute's namespace was declared somewhere above")
}

fn write_naive_events(found: &[Event]) -> String {
    use core::fmt::Write as _;
    let mut out = String::new();
    let mut scopes: Vec<Vec<(Option<String>, String)>> = Vec::new();
    for event in found {
        match event {
            Event::StartElement {
                name,
                prefix,
                attributes,
                namespaces,
                ..
            } => {
                scopes.push(namespaces.clone());
                let _ =
                    write!(out, "<{}", qname(prefix.as_deref(), &name.local));
                for (p, uri) in namespaces {
                    match p {
                        Some(p) => {
                            let _ = write!(
                                out,
                                " xmlns:{p}=\"{}\"",
                                escape_xml(uri)
                            );
                        }
                        None => {
                            let _ =
                                write!(out, " xmlns=\"{}\"", escape_xml(uri));
                        }
                    }
                }
                for (attr, value) in attributes {
                    let p = attr
                        .namespace
                        .as_deref()
                        .map(|ns| prefix_for(&scopes, ns));
                    let _ = write!(
                        out,
                        " {}=\"{}\"",
                        qname(p.as_deref(), &attr.local),
                        escape_xml(value)
                    );
                }
                out.push('>');
            }
            Event::EndElement { name, prefix, .. } => {
                let _ = scopes.pop();
                let _ =
                    write!(out, "</{}>", qname(prefix.as_deref(), &name.local));
            }
            Event::Text(t) => out.push_str(&escape_xml(t)),
            Event::Comment(c) => {
                let _ = write!(out, "<!--{c}-->");
            }
            Event::ProcessingInstruction { target, data } => {
                let _ = write!(out, "<?{target} {data}?>");
            }
            _ => panic!("an event this writer does not know"),
        }
    }
    out
}

#[test]
fn start_events_carry_enough_to_rewrite_the_document() {
    for doc in [
        r#"<r xmlns="urn:d" xmlns:p="urn:p"><p:a p:x="1" y="2">t &amp; u</p:a><b/></r>"#,
        r#"<?pi data?><r><s xmlns:q="urn:q" q:k="v"><q:t/></s><!-- c --></r>"#,
        r#"<p:r xmlns:p="urn:p" xmlns:q="urn:p"><q:a q:b='1'/></p:r>"#,
    ] {
        let original = events(doc).expect("valid");
        let written = write_naive_events(&original);
        let reread = events(&written).unwrap_or_else(|e| {
            panic!("the rewritten {written:?} does not read: {e}")
        });
        assert_eq!(reread, original, "{doc:?} rewritten as {written:?}");
    }
    // The element's own prefix is the one it was written with, even
    // when two prefixes name one namespace.
    let found = events(r#"<p:r xmlns:p="urn:p" xmlns:q="urn:p"><q:a/></p:r>"#)
        .expect("valid");
    let prefixes: Vec<Option<String>> = found
        .iter()
        .filter_map(|e| match e {
            Event::StartElement { prefix, .. }
            | Event::EndElement { prefix, .. } => Some(prefix.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        prefixes,
        [
            Some("p".into()),
            Some("q".into()),
            Some("q".into()),
            Some("p".into())
        ]
    );
}

/// `offset()` is where the scanner will resume: the start of the
/// next construct, which is where the tree parser reports an error
/// that construct is to blame for.
#[test]
fn offset_is_where_the_tree_parser_would_report_the_next_error() {
    for doc in [
        "<a><b></c></a>",
        "<a><b/>&undefined;</a>",
        "<a/><b/>",
        "<a><!-- c --><b x='1' x='2'/></a>",
        "<a>text</b>",
    ] {
        let tree = parse(doc).expect_err("malformed");
        let mut reader = Reader::new(doc).expect("prolog is fine");
        let mut before_error = reader.offset();
        let error = loop {
            match reader.next_event() {
                Ok(Some(_)) => before_error = reader.offset(),
                Ok(None) => panic!("{doc:?} must fail"),
                Err(e) => break e,
            }
        };
        assert_eq!(error.offset, tree.offset, "{doc:?}: the same error");
        assert_eq!(
            before_error, tree.offset,
            "{doc:?}: offset() pointed at it"
        );
    }
    // And on a complete document, it ends where the document does.
    let doc = "<a><b/></a>";
    let mut reader = Reader::new(doc).expect("valid");
    assert_eq!(reader.offset(), 0);
    let _ = reader.next_event().expect("<a>");
    assert_eq!(reader.offset(), 3);
    let _ = reader.next_event().expect("<b/>");
    assert_eq!(reader.offset(), 7, "a self-closing tag is passed whole");
    let _ = reader.next_event().expect("</b>, held back");
    assert_eq!(reader.offset(), 7);
    while reader.next_event().expect("valid").is_some() {}
    assert_eq!(reader.offset(), doc.len());
}

/// An external source given to the reader reaches the DTD parser, as
/// it does through `parse_with_external`.
#[test]
fn an_external_source_supplies_the_external_subset() {
    struct Dtd;
    impl oxml::external::ExternalSource for Dtd {
        fn fetch(&self, system: &str, _public: Option<&str>) -> Option<&str> {
            (system == "x.dtd").then_some("<!ENTITY e \"from outside\">")
        }
    }
    let doc = "<!DOCTYPE a SYSTEM \"x.dtd\"><a>&e;</a>";
    let source = Dtd;
    let text: String = Reader::new(doc)
        .expect("valid")
        .with_external(&source)
        .filter_map(|e| match e {
            Ok(Event::Text(t)) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(text, "from outside");
    let tree = oxml::parse_with_external(doc, oxml::Limits::default(), &source)
        .expect("valid");
    assert_eq!(tree.text(tree.root()), text, "as the tree parser has it");
    // Without one the subset is unavailable, and a reference to an
    // entity it might have declared expands to nothing -- on both
    // paths.
    let without: String = events(doc)
        .expect("not an error while the subset is unknown")
        .into_iter()
        .filter_map(|e| match e {
            Event::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(without, "");
    assert_eq!(parse(doc).expect("valid").text(tree.root()), "");
}
