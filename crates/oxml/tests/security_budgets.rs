// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Regression tests for resource accounting at parser boundaries.

use oxml::{ErrorKind, Limits, parse_with};

#[test]
fn dtd_groups_do_not_exhaust_the_stack() {
    const CHILD: &str = "OXML_DTD_STACK_PROBE";
    if std::env::var_os(CHILD).is_some() {
        let input = format!(
            "<!DOCTYPE a [<!ELEMENT a {}b{}>]><a/>",
            "(".repeat(30_000),
            ")".repeat(30_000)
        );
        assert!(parse_with(&input, Limits::strict()).is_err());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "dtd_groups_do_not_exhaust_the_stack"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "DTD nesting must return an error: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn node_budget_covers_prolog_and_epilog() {
    let mut limits = Limits::strict();
    limits.max_nodes = Some(4);
    for input in [
        format!("{}<a/>", "<!--x-->".repeat(10)),
        format!("<a/>{}", "<?p x?>".repeat(10)),
    ] {
        assert_eq!(
            parse_with(&input, limits).unwrap_err().kind,
            ErrorKind::TooManyNodes
        );
    }
}

#[test]
fn node_budget_covers_self_closing_attributes() {
    let mut limits = Limits::strict();
    limits.max_nodes = Some(4);
    assert_eq!(
        parse_with("<a x='1' y='2' z='3'/>", limits)
            .unwrap_err()
            .kind,
        ErrorKind::TooManyNodes
    );
}

#[test]
fn complete_stream_constructs_are_bounded() {
    let mut limits = Limits::strict();
    limits.max_construct_bytes = Some(16);
    let mut core = oxml::stream::Core::with_limits(limits);
    core.feed(b"<root><a/>").unwrap();
    let mut events = 0;
    while core.next_event().unwrap().is_some() {
        events += 1;
    }
    assert_eq!(events, 3);
    let input = format!("<!--{}-->", "x".repeat(1000));
    assert_eq!(
        core.feed(input.as_bytes()).unwrap_err().kind,
        ErrorKind::ConstructTooLarge
    );
}

#[test]
fn source_and_decoded_bytes_are_separate_budgets() {
    let input = "<a>éé</a>";
    let mut limits = Limits::default();
    limits.max_input_bytes = Some(input.len() - 1);
    assert!(parse_with(input, limits).is_err());
    limits.max_input_bytes = Some(input.len());
    assert!(parse_with(input, limits).is_ok());
    limits.max_document_bytes = Some(input.len() - 1);
    assert!(parse_with(input, limits).is_err());
}

#[cfg(feature = "std")]
#[test]
fn stream_cap_measures_constructs_not_source_chunks() {
    let mut limits = Limits::default();
    limits.max_construct_bytes = Some(16);
    let input = format!("<r>{}</r>", "<a/>".repeat(100));
    let reader =
        oxml::stream::Reader::from_reader_with(input.as_bytes(), limits)
            .unwrap();
    assert_eq!(reader.collect::<oxml::Result<Vec<_>>>().unwrap().len(), 202);
}

#[test]
fn stream_cap_covers_later_constructs_after_eof() {
    let mut limits = Limits::default();
    limits.max_construct_bytes = Some(16);
    let mut core = oxml::stream::Core::with_limits(limits);
    core.feed(b"<r><a/>").unwrap();
    while core.next_event().unwrap().is_some() {}
    let input = format!("<b/><!--{}--></r>", "x".repeat(100));
    core.feed(input.as_bytes()).unwrap();
    core.feed_eof().unwrap();
    assert!(core.next_event().unwrap().is_some());
    assert!(core.next_event().unwrap().is_some());
    assert_eq!(
        core.next_event().unwrap_err().kind,
        ErrorKind::ConstructTooLarge
    );
}

#[test]
fn decoded_byte_budget_covers_transcoding() {
    let input = "<?xml version='1.0' encoding='iso-8859-1'?><a>éé</a>";
    let bytes: Vec<_> = input.chars().map(|c| c as u8).collect();
    let mut limits = Limits::default();
    limits.max_input_bytes = Some(bytes.len());
    limits.max_document_bytes = Some(input.len() - 1);
    assert!(oxml::parse_bytes_with(&bytes, limits).is_err());
    limits.max_document_bytes = Some(input.len());
    assert!(oxml::parse_bytes_with(&bytes, limits).is_ok());
}

#[cfg(feature = "xpath")]
#[test]
fn xpath_arguments_and_literals_have_independent_budgets() {
    let mut limits = Limits::default();
    limits.max_xpath_arguments = 2;
    assert!(
        oxml::XPath::compile_with_limits("concat('a','b','c')", &limits)
            .is_err()
    );
    limits.max_xpath_arguments = 3;
    assert!(
        oxml::XPath::compile_with_limits("concat('a','b','c')", &limits)
            .is_ok()
    );
    limits.max_xpath_nodes = 2;
    assert!(
        oxml::XPath::compile_with_limits("concat('a','b')", &limits).is_err()
    );
    limits.max_xpath_bytes = Some(4);
    assert!(oxml::XPath::compile_with_limits("'abcd'", &limits).is_err());
}

#[cfg(feature = "xpath")]
#[test]
fn xpath_evaluation_stops_at_its_work_budget() {
    use oxml::xpath::{EvalContext, EvalLimits};
    let doc = oxml::parse(&format!("<r>{}</r>", "<a/>".repeat(100))).unwrap();
    let xp = oxml::XPath::compile("//a[following::a]").unwrap();
    let mut limits = EvalLimits::default();
    limits.max_work = 10_000;
    let context = EvalContext::new().with_limits(limits);
    assert!(
        oxml::XPath::compile("true()")
            .unwrap()
            .evaluate_with(&doc, &context)
            .is_ok()
    );
    assert!(xp.evaluate_with(&doc, &context).is_err());
    assert_eq!(xp.evaluate(&doc).nodes().unwrap().len(), 99);
}

#[cfg(feature = "xpath")]
#[test]
fn xpath_evaluation_bounds_materialized_strings() {
    use oxml::xpath::{EvalContext, EvalLimits};
    let doc = oxml::parse("<r>abcdef</r>").unwrap();
    let xp = oxml::XPath::compile("string(/r)").unwrap();
    let mut limits = EvalLimits::default();
    limits.max_string_bytes = 5;
    let context = EvalContext::new().with_limits(limits);
    assert!(xp.evaluate_with(&doc, &context).is_err());
}

#[test]
fn tree_and_stream_charge_the_same_node_reservations() {
    for source in [
        "<!--x--><a/><?p x?>",
        "<a x='1' xmlns:p='u'/>",
        "<a>x<b/></a>",
        "<a><![CDATA[]]><b/></a>",
        "<a> &amp; </a>",
    ] {
        for max in 0..8 {
            let mut limits = Limits::default();
            limits.max_nodes = Some(max);
            let tree = parse_with(source, limits);
            let stream = oxml::stream::Reader::with_limits(source, limits)
                .and_then(Iterator::collect::<oxml::Result<Vec<_>>>);
            assert_eq!(
                tree.as_ref().err(),
                stream.as_ref().err(),
                "{source}: {max}"
            );
            if let Ok(tree) = tree {
                assert!(tree.len() <= max);
            }
        }
    }
}

#[test]
fn dtd_groups_and_conditional_sections_have_their_own_depth_budget() {
    let mut limits = Limits::default();
    limits.max_dtd_depth = 0;
    for model in ["(a)", "(#PCDATA)", "(#PCDATA|a)*"] {
        let source = format!("<!DOCTYPE r [<!ELEMENT r {model}>]><r/>");
        assert_eq!(
            parse_with(&source, limits).unwrap_err().kind,
            ErrorKind::DtdDepthLimitExceeded
        );
    }
    limits.max_dtd_depth = 2;
    assert!(
        parse_with("<!DOCTYPE r [<!ELEMENT r ((a))>]><r/>", limits).is_ok()
    );
    let too_deep = "<!DOCTYPE r [<!ELEMENT r (((a)))>]><r/>";
    assert_eq!(
        parse_with(too_deep, limits).unwrap_err().kind,
        ErrorKind::DtdDepthLimitExceeded
    );
    let subset = format!(
        "{}<!ELEMENT r EMPTY>{}",
        "<![INCLUDE[".repeat(3),
        "]]>".repeat(3)
    );
    let parts: &[(&str, &str)] = &[("parts.dtd", &subset)];
    let error = oxml::parse_with_external(
        "<!DOCTYPE r SYSTEM 'parts.dtd'><r/>",
        limits,
        &parts,
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::DtdDepthLimitExceeded);
}

#[test]
fn utf16_transcoding_refuses_a_decoded_byte_overrun() {
    let text = "<r>界界</r>";
    let bytes: Vec<_> = [0xff, 0xfe]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let mut limits = Limits::default();
    limits.max_document_bytes = Some(text.len() - 1);
    assert_eq!(
        oxml::parse_bytes_with(&bytes, limits).unwrap_err().kind,
        ErrorKind::DocumentTooLarge
    );
    limits.max_document_bytes = Some(text.len());
    assert!(oxml::parse_bytes_with(&bytes, limits).is_ok());
}

#[test]
fn incremental_source_bytes_are_charged_across_feeds() {
    let mut limits = Limits::default();
    limits.max_input_bytes = Some(10);
    let mut core = oxml::stream::Core::with_limits(limits);
    core.feed(b"<r><a/>").unwrap();
    while core.next_event().unwrap().is_some() {}
    assert_eq!(
        core.feed(b"</r>").unwrap_err().kind,
        ErrorKind::InputTooLarge
    );
}

#[cfg(feature = "std")]
#[test]
fn decoded_budget_is_charged_before_line_ending_normalization() {
    let text = "<?xml version='1.1'?><r>\r\n\u{2028}</r>";
    let utf16: Vec<_> = [0xff, 0xfe]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let mut limits = Limits::default();
    for bytes in [text.as_bytes(), utf16.as_slice()] {
        limits.max_document_bytes = Some(text.len() - 1);
        assert_eq!(
            oxml::parse_bytes_with(bytes, limits).unwrap_err().kind,
            ErrorKind::DocumentTooLarge
        );
        for chunk in [1, 3, bytes.len()] {
            let source = std::io::BufReader::with_capacity(chunk, bytes);
            let stream = oxml::stream::Reader::from_reader_with(source, limits)
                .and_then(Iterator::collect::<oxml::Result<Vec<_>>>);
            assert_eq!(stream.unwrap_err().kind, ErrorKind::DocumentTooLarge);
        }
        limits.max_document_bytes = Some(text.len());
        let stream = oxml::stream::Reader::from_reader_with(bytes, limits)
            .and_then(Iterator::collect::<oxml::Result<Vec<_>>>);
        assert!(stream.is_ok());
    }
}

#[cfg(feature = "xpath")]
#[test]
fn bounded_xpath_preserves_answers_and_resets_per_evaluation() {
    use oxml::xpath::{EvalContext, EvalLimits, Value};
    let doc = oxml::parse("<!DOCTYPE r [<!ATTLIST a id ID #IMPLIED>]><r xml:lang='en'><a id='one'>2</a><b>3</b><a>4</a></r>").unwrap();
    let context = EvalContext::new().with_limits(EvalLimits::default()).bind(
        oxml::ExpandedName::local("x"),
        Value::String("hello".into()),
    );
    for source in [
        "/r/a | /r/b",
        "/r/a/preceding::*",
        "/r/a/following::*",
        "/r/namespace::*",
        "string(/r)",
        "sum(/r/a)",
        "name(/r)",
        "normalize-space(' a  b ')",
        "translate('aaé','aé','界x')",
        "substring('abcdef',2,3)",
        "substring-before('abc','b')",
        "substring-after('abc','b')",
        "concat('a','b')",
        "number(/r/b)",
        "/r/a = /r/b",
        "/r/a != '8'",
        "/r/a < /r/b",
        "/r/a[lang('en')]",
        "id('one')",
        "id(/r/a/@id)",
        "$x",
        "string-length($x)",
    ] {
        let query = oxml::XPath::compile(source).unwrap();
        let expected = query
            .evaluate_with(
                &doc,
                &EvalContext::new().bind(
                    oxml::ExpandedName::local("x"),
                    Value::String("hello".into()),
                ),
            )
            .unwrap();
        assert_eq!(
            query.evaluate_with(&doc, &context).unwrap(),
            expected,
            "{source}"
        );
        assert_eq!(
            query.evaluate_with(&doc, &context).unwrap(),
            expected,
            "fresh budget: {source}"
        );
    }
}

#[cfg(feature = "xpath")]
#[test]
fn bounded_xpath_rejects_large_or_stale_variable_sets_before_copying() {
    use oxml::xpath::{EvalContext, EvalError, EvalLimits, Value};
    let mut doc = oxml::parse("<r><a/></r>").unwrap();
    let mut limits = EvalLimits::default();
    limits.max_nodes = 4;
    let query = oxml::XPath::compile("$nodes").unwrap();
    let context = EvalContext::new().with_limits(limits).bind(
        oxml::ExpandedName::local("nodes"),
        Value::NodeSet(vec![doc.root(); 5]),
    );
    assert_eq!(
        query.evaluate_with(&doc, &context),
        Err(EvalError::LimitExceeded("nodes"))
    );
    let node = doc.children(doc.root_element().unwrap())[0];
    doc.remove(node).unwrap();
    let context = EvalContext::new().with_limits(limits).bind(
        oxml::ExpandedName::local("nodes"),
        Value::NodeSet(vec![node]),
    );
    assert_eq!(query.evaluate_with(&doc, &context), Err(EvalError::Stale));
}

#[cfg(feature = "xpath")]
#[test]
fn name_function_budgets_include_namespace_prefixes_and_pi_targets() {
    use oxml::xpath::{EvalContext, EvalError, EvalLimits};
    let mut doc = oxml::Document::empty();
    let root = doc.append_element(doc.root(), None, "r").unwrap();
    let long = "p".repeat(64);
    let _ = doc.declare_namespace(root, Some(&long), "urn:u").unwrap();
    let _ = doc.append_pi(root, &long, "").unwrap();
    let mut limits = EvalLimits::default();
    limits.max_string_bytes = 5;
    let context = EvalContext::new().with_limits(limits);
    for source in ["name(/r/namespace::*)", "name(//processing-instruction())"]
    {
        let query = oxml::XPath::compile(source).unwrap();
        assert_eq!(query.evaluate(&doc).to_str(&doc), long);
        assert_eq!(
            query.evaluate_with(&doc, &context),
            Err(EvalError::LimitExceeded("string bytes"))
        );
    }
}
