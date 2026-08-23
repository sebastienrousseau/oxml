// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Property-based tests.
//!
//! Example-based tests check the cases someone thought of. These state
//! invariants that must hold for *every* input, and let proptest search
//! for the counterexample. Failures are recorded in
//! `proptest-regressions/` and replayed on every later run, so a bug
//! found once is tested for forever.

use oxml::{Limits, NodeKind, XPath, parse, parse_with};
use proptest::prelude::*;

/// Arbitrary bytes, biased towards things that look like XML.
fn xml_ish() -> impl Strategy<Value = String> {
    prop_oneof![
        // Free-form: finds the shapes nobody would write by hand.
        2 => "\\PC{0,200}",
        // Fragments assembled from real tokens: gets past the parser's
        // first byte, which free-form text rarely does.
        3 => proptest::collection::vec(
            prop_oneof![
                Just("<a>".to_owned()),
                Just("</a>".to_owned()),
                Just("<b/>".to_owned()),
                Just("<a x=\"1\">".to_owned()),
                Just("text".to_owned()),
                Just("<!--c-->".to_owned()),
                Just("<![CDATA[d]]>".to_owned()),
                Just("&amp;".to_owned()),
                Just("&#65;".to_owned()),
                Just("<?pi?>".to_owned()),
                Just(" ".to_owned()),
            ],
            0..24,
        ).prop_map(|parts| parts.concat()),
    ]
}

/// A well-formed document, built by construction.
fn well_formed() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        Just("<e/>".to_owned()),
        Just("<e>t</e>".to_owned()),
        Just("<e a=\"v\">t</e>".to_owned()),
        Just("<e><!--c--></e>".to_owned()),
    ];
    leaf.prop_recursive(4, 32, 3, |inner| {
        proptest::collection::vec(inner, 0..3)
            .prop_map(|kids| format!("<n>{}</n>", kids.concat()))
    })
}

proptest! {
    /// The parser is total: it returns, whatever it is given.
    #[test]
    fn parsing_never_panics(src in xml_ish()) {
        let _ = parse(&src);
    }

    /// Tightening a limit can only ever reject more, never accept more.
    #[test]
    fn tighter_limits_never_accept_more(src in xml_ish()) {
        if parse_with(&src, Limits::strict()).is_ok() {
            prop_assert!(parse_with(&src, Limits::default()).is_ok());
            prop_assert!(parse_with(&src, Limits::permissive()).is_ok());
        }
    }

    /// Parsing is deterministic — same input, same tree shape.
    #[test]
    fn parsing_is_deterministic(src in xml_ish()) {
        match (parse(&src), parse(&src)) {
            (Ok(a), Ok(b)) => {
                prop_assert_eq!(a.len(), b.len());
                prop_assert_eq!(a.text(a.root()), b.text(b.root()));
            }
            (Err(a), Err(b)) => {
                prop_assert_eq!(a.offset, b.offset);
            }
            _ => prop_assert!(false, "one parse succeeded and one failed"),
        }
    }

    /// Anything `well_formed` builds must actually parse. If this
    /// fails, either the generator or the parser is wrong — and both
    /// are worth knowing.
    #[test]
    fn constructed_documents_parse(src in well_formed()) {
        prop_assert!(parse(&src).is_ok(), "generator produced: {}", src);
    }

    /// Every tree that parses is internally coherent.
    #[test]
    fn trees_are_coherent(src in well_formed()) {
        let doc = parse(&src).expect("well-formed by construction");
        prop_assert_eq!(doc.parent(doc.root()), None);

        for id in doc.descendants() {
            for child in doc.children(id) {
                prop_assert_eq!(doc.parent(*child), Some(id));
            }
            // Attributes are reachable but are never children.
            for attr in doc.attribute_nodes(id) {
                prop_assert_eq!(doc.parent(*attr), Some(id));
                prop_assert!(!doc.children(id).contains(attr));
            }
            // Adjacent text is coalesced during parsing.
            for pair in doc.children(id).windows(2) {
                let a = matches!(doc.kind(pair[0]), Some(NodeKind::Text(_)));
                let b = matches!(doc.kind(pair[1]), Some(NodeKind::Text(_)));
                prop_assert!(!(a && b), "adjacent text nodes");
            }
        }
    }

    /// Walking up from any node reaches the root in finite steps.
    #[test]
    fn parent_chains_terminate(src in well_formed()) {
        let doc = parse(&src).expect("well-formed");
        for id in doc.descendants() {
            let mut hops = 0usize;
            let mut cur = Some(id);
            while let Some(n) = cur {
                cur = doc.parent(n);
                hops += 1;
                prop_assert!(hops <= doc.len(), "chain does not terminate");
            }
        }
    }

    /// XPath compilation is total and deterministic.
    #[test]
    fn xpath_compilation_never_panics(expr in "\\PC{0,60}") {
        if let Ok(a) = XPath::compile(&expr) {
            let b = XPath::compile(&expr).expect("compiled once already");
            prop_assert_eq!(format!("{:?}", a.expr()), format!("{:?}", b.expr()));
        }
    }

    /// `count(X)` agrees with the number of nodes `X` selects. Two
    /// independent paths through the evaluator that must not diverge.
    #[test]
    fn count_agrees_with_the_node_set(src in well_formed()) {
        let doc = parse(&src).expect("well-formed");
        for path in ["//n", "//e", "//e/@a", "//text()", "//comment()"] {
            let nodes = XPath::compile(path)
                .expect("fixed expression")
                .evaluate(&doc)
                .nodes()
                .map_or(0, <[oxml::NodeId]>::len);
            let counted = XPath::compile(&format!("count({path})"))
                .expect("fixed expression")
                .evaluate(&doc)
                .to_str(&doc);
            prop_assert_eq!(counted, nodes.to_string(), "for {}", path);
        }
    }

    /// Number formatting round-trips: whatever XPath prints must parse
    /// back to the same value. This is the property the `log10`/`powf`
    /// removal was about — it must hold identically on every backend.
    #[test]
    fn formatted_numbers_round_trip(n in proptest::num::f64::NORMAL) {
        let doc = parse("<a/>").expect("well-formed");
        let expr = format!("{n:?} * 1");
        let Ok(x) = XPath::compile(&expr) else { return Ok(()) };
        let printed = x.evaluate(&doc).to_str(&doc);
        if let Ok(back) = printed.parse::<f64>() {
            // 15 significant digits, so allow the last two to differ.
            let tolerance = n.abs() * 1e-13;
            prop_assert!(
                (back - n).abs() <= tolerance.max(f64::MIN_POSITIVE),
                "{n} printed as {printed}, parsed back as {back}"
            );
        }
    }
}
