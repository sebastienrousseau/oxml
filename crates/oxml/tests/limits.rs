// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Resource bounds.
//!
//! Every limit is exercised three ways: input just under it is
//! accepted, input just over it is rejected, and the rejection carries
//! the *specific* [`ErrorKind`] for that bound rather than a generic
//! failure. A limit that rejects with the wrong kind is as good as
//! absent — the caller cannot tell what to change.

use std::fmt::Write as _;

use oxml::{ErrorKind, Limits, parse, parse_with};

/// `n` distinct attributes, as a single string.
fn many_attributes(n: usize) -> String {
    (0..n).fold(String::new(), |mut acc, i| {
        let _ = write!(acc, " a{i}=\"v\"");
        acc
    })
}

fn err_kind(src: &str, limits: Limits) -> ErrorKind {
    parse_with(src, limits)
        .expect_err("should exceed a limit")
        .kind
}

#[test]
fn the_defaults_accept_an_ordinary_document() {
    let doc = "<library><book id=\"1\" lang=\"en\">\
               <title>Dune</title></book></library>";
    assert!(parse(doc).is_ok());
    assert!(parse_with(doc, Limits::default()).is_ok());
    assert!(parse_with(doc, Limits::strict()).is_ok());
    assert!(parse_with(doc, Limits::permissive()).is_ok());
}

#[test]
fn parse_and_parse_with_default_agree() {
    for src in ["<a/>", "<a><b>x</b></a>", "<a x=\"1\"/>", "<a><!--c--></a>"] {
        assert_eq!(
            parse(src).is_ok(),
            parse_with(src, Limits::default()).is_ok(),
            "{src}"
        );
    }
}

#[test]
fn max_depth_is_enforced_at_its_boundary() {
    let mut l = Limits::default();
    l.max_depth = 8;

    let ok = format!("{}{}", "<a>".repeat(8), "</a>".repeat(8));
    assert!(parse_with(&ok, l).is_ok(), "8 deep with a limit of 8");

    let over = format!("{}{}", "<a>".repeat(9), "</a>".repeat(9));
    assert_eq!(err_kind(&over, l), ErrorKind::DepthLimitExceeded);
}

#[test]
fn max_attributes_per_element_is_enforced() {
    let mut l = Limits::default();
    l.max_attributes_per_element = 4;

    let attrs = |n: usize| format!("<e{}/>", many_attributes(n));

    assert!(parse_with(&attrs(4), l).is_ok(), "4 with a limit of 4");
    assert_eq!(err_kind(&attrs(5), l), ErrorKind::TooManyAttributes);
}

#[test]
fn max_attribute_size_is_enforced() {
    let mut l = Limits::default();
    l.max_attribute_size = 16;

    let ok = format!("<e a=\"{}\"/>", "x".repeat(16));
    assert!(parse_with(&ok, l).is_ok());

    let over = format!("<e a=\"{}\"/>", "x".repeat(17));
    assert_eq!(err_kind(&over, l), ErrorKind::AttributeTooLarge);
}

#[test]
fn max_name_length_is_enforced_on_elements_and_attributes() {
    let mut l = Limits::default();
    l.max_name_length = 8;

    assert!(parse_with(&format!("<{}/>", "e".repeat(8)), l).is_ok());
    assert_eq!(
        err_kind(&format!("<{}/>", "e".repeat(9)), l),
        ErrorKind::NameTooLong,
        "element name"
    );
    assert_eq!(
        err_kind(&format!("<e {}=\"v\"/>", "a".repeat(9)), l),
        ErrorKind::NameTooLong,
        "attribute name"
    );
}

#[test]
fn max_nodes_is_enforced() {
    let mut l = Limits::default();
    l.max_nodes = Some(10);

    assert!(parse_with("<a><b/></a>", l).is_ok());

    let many = format!("<a>{}</a>", "<b/>".repeat(1000));
    assert_eq!(err_kind(&many, l), ErrorKind::TooManyNodes);
}

#[test]
fn max_text_length_is_enforced() {
    let mut l = Limits::default();
    l.max_text_length = Some(16);

    assert!(parse_with(&format!("<a>{}</a>", "x".repeat(16)), l).is_ok());
    assert_eq!(
        err_kind(&format!("<a>{}</a>", "x".repeat(17)), l),
        ErrorKind::TextTooLong
    );
}

#[test]
fn text_length_counts_the_whole_coalesced_run() {
    // Adjacent text and entity references merge into one node, so a
    // limit applied per-fragment would be trivially bypassed by
    // splitting the payload with `&amp;`.
    let mut l = Limits::default();
    l.max_text_length = Some(16);

    let split = format!("<a>{}&amp;{}</a>", "x".repeat(10), "y".repeat(10));
    assert_eq!(err_kind(&split, l), ErrorKind::TextTooLong);
}

#[test]
fn strict_limits_reject_what_defaults_accept() {
    // Otherwise `strict()` is decorative.
    let deep = format!("{}{}", "<a>".repeat(100), "</a>".repeat(100));
    assert!(parse_with(&deep, Limits::default()).is_ok());
    assert_eq!(
        err_kind(&deep, Limits::strict()),
        ErrorKind::DepthLimitExceeded
    );
}

#[test]
fn permissive_limits_accept_what_defaults_reject() {
    // Not depth — that one is bounded by the stack, not by policy.
    let wide = (0..5_000).fold(String::from("<e"), |mut acc, i| {
        let _ = write!(acc, " a{i}=\"v\"");
        acc
    }) + "/>";
    assert_eq!(
        err_kind(&wide, Limits::default()),
        ErrorKind::TooManyAttributes
    );
    assert!(parse_with(&wide, Limits::permissive()).is_ok());
}

#[test]
fn permissive_does_not_raise_the_depth_limit() {
    // Raising it would hand out a stack overflow, which aborts the
    // process rather than returning an error — the opposite of what a
    // limit is for. Measured ceiling in a debug build on a 2 MiB test
    // thread is ~280 levels, so anything generous here is a crash.
    assert_eq!(
        Limits::permissive().max_depth,
        Limits::default().max_depth,
        "permissive() must not raise max_depth past the stack ceiling"
    );
}

#[test]
fn the_default_depth_limit_stays_under_the_debug_stack_ceiling() {
    // A debug build on a 2 MiB thread (what `cargo test` provides)
    // survives ~280 levels at ~7.5 KiB per frame. If a change grows the
    // frame, this fails here rather than aborting the test process.
    assert!(
        Limits::default().max_depth <= 280,
        "default max_depth {} exceeds the measured debug ceiling",
        Limits::default().max_depth
    );
    let at_limit = format!(
        "{}{}",
        "<a>".repeat(Limits::default().max_depth),
        "</a>".repeat(Limits::default().max_depth)
    );
    assert!(parse(&at_limit).is_ok(), "the default must be reachable");
}

#[test]
fn limits_are_copy_and_adjustable_from_default() {
    // The type is `#[non_exhaustive]`, so this is the supported way to
    // build one. If `Copy` were lost this stops compiling.
    let mut l = Limits::default();
    l.max_depth = 4;
    let copy = l;
    assert_eq!(copy.max_depth, 4);
    assert_eq!(l, copy);
}

#[test]
fn a_hostile_document_fails_fast_under_strict_limits() {
    // The shapes that only appear in attacks.
    let cases = [
        format!("{}{}", "<a>".repeat(100_000), "</a>".repeat(100_000)),
        format!("<e{}/>", many_attributes(100_000)),
        format!("<a>{}</a>", "<b/>".repeat(1_000_000)),
        format!("<a>{}</a>", "x".repeat(50_000_000)),
    ];
    for (i, src) in cases.iter().enumerate() {
        assert!(
            parse_with(src, Limits::strict()).is_err(),
            "case {i} was accepted"
        );
    }
}
