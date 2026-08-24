// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The `XPath` 1.0 function library.
//!
//! Each function has a specified return type and a specified behaviour
//! on the awkward inputs — an empty node-set, a negative index, a
//! non-numeric string. Those are the cases callers hit in practice and
//! the ones where implementations quietly diverge.

use oxml::{XPath, parse};

const DOC: &str = "\
<r xmlns:m=\"urn:m\">\
<a id=\"1\">  spaced   out  </a>\
<b>12</b><b>30</b>\
<m:tagged>ns</m:tagged>\
<empty/>\
</r>";

fn val(expr: &str) -> String {
    // `m` is bound for every expression here, because a prefix in an
    // expression resolves against the expression's own bindings and
    // not the document's declarations -- an unbound one is a compile
    // error rather than a match on the local part alone.
    let doc = parse(DOC).expect("well-formed");
    XPath::compile_with_namespaces(expr, &[("m", "urn:m")])
        .unwrap_or_else(|e| panic!("`{expr}` failed to compile: {e}"))
        .evaluate(&doc)
        .to_str(&doc)
}

#[test]
fn string_conversion_follows_the_type_rules() {
    assert_eq!(val("string(42)"), "42");
    assert_eq!(val("string(4.5)"), "4.5");
    assert_eq!(val("string(true())"), "true");
    assert_eq!(val("string(false())"), "false");
    assert_eq!(val("string('x')"), "x");
    assert_eq!(val("string(//b)"), "12", "the first node in the set");
    assert_eq!(
        val("string(//missing)"),
        "",
        "an empty set is an empty string"
    );
}

#[test]
fn number_conversion_yields_nan_for_non_numeric_input() {
    assert_eq!(val("number('42')"), "42");
    assert_eq!(val("number('4.5')"), "4.5");
    assert_eq!(val("number(true())"), "1");
    assert_eq!(val("number(false())"), "0");
    assert_eq!(val("number('abc')"), "NaN");
    assert_eq!(val("number(//missing)"), "NaN");
}

#[test]
fn count_and_sum_operate_on_node_sets() {
    assert_eq!(val("count(//b)"), "2");
    assert_eq!(val("count(//missing)"), "0");
    assert_eq!(val("sum(//b)"), "42");
    assert_eq!(val("sum(//missing)"), "0");
}

#[test]
fn string_length_counts_characters() {
    assert_eq!(val("string-length('abc')"), "3");
    assert_eq!(val("string-length('')"), "0");
    assert_eq!(val("string-length('é中')"), "2", "characters, not bytes");
}

#[test]
fn concat_joins_every_argument() {
    assert_eq!(val("concat('a','b')"), "ab");
    assert_eq!(val("concat('a','b','c','d')"), "abcd");
    assert_eq!(val("concat('n=', 1)"), "n=1");
}

#[test]
fn starts_with_and_contains_answer_booleans() {
    assert_eq!(val("starts-with('abcdef','abc')"), "true");
    assert_eq!(val("starts-with('abcdef','bcd')"), "false");
    assert_eq!(val("starts-with('abc','')"), "true");
    assert_eq!(val("contains('abcdef','cde')"), "true");
    assert_eq!(val("contains('abcdef','xyz')"), "false");
    assert_eq!(val("contains('abc','')"), "true");
}

#[test]
fn normalize_space_collapses_and_trims() {
    assert_eq!(val("normalize-space('  a   b  ')"), "a b");
    assert_eq!(val("normalize-space('')"), "");
    assert_eq!(val("normalize-space('   ')"), "");
    assert_eq!(val("normalize-space(//a)"), "spaced out");
}

#[test]
fn substring_is_one_based_and_clamps_out_of_range_indices() {
    // XPath counts from 1, and the specification requires out-of-range
    // requests to be clamped rather than to error.
    assert_eq!(val("substring('12345',2,3)"), "234");
    assert_eq!(val("substring('12345',2)"), "2345");
    assert_eq!(val("substring('12345',0,3)"), "12");
    assert_eq!(val("substring('12345',-1,3)"), "1");
    assert_eq!(val("substring('12345',10)"), "");
    assert_eq!(val("substring('12345',1,0)"), "");
    assert_eq!(val("substring('12345',1,100)"), "12345");
}

#[test]
fn local_name_and_namespace_uri_read_the_context_node() {
    assert_eq!(val("local-name(//m:tagged)"), "tagged");
    assert_eq!(val("namespace-uri(//m:tagged)"), "urn:m");
    assert_eq!(val("local-name(//a)"), "a");
    assert_eq!(val("namespace-uri(//a)"), "", "no namespace");
    assert_eq!(val("local-name(//missing)"), "", "an empty set has no name");
}

#[test]
fn the_rounding_functions_follow_the_specification() {
    assert_eq!(val("floor(1.9)"), "1");
    assert_eq!(val("floor(-1.1)"), "-2");
    assert_eq!(val("ceiling(1.1)"), "2");
    assert_eq!(val("ceiling(-1.9)"), "-1");
    assert_eq!(val("round(1.5)"), "2");
    assert_eq!(val("round(1.4)"), "1");
    assert_eq!(val("round(-1.5)"), "-1", "XPath rounds half towards +∞");
}

#[test]
fn evaluate_from_uses_the_supplied_context_node() {
    // The whole point of the entry point: a relative expression means
    // something different from a different node.
    let doc = parse(DOC).expect("well-formed");
    let books = XPath::compile("//b").expect("valid").evaluate(&doc);
    let nodes = books.nodes().expect("a node set");
    let second = nodes[1];

    let here = XPath::compile("self::b").expect("valid");
    assert_eq!(here.evaluate_from(&doc, second).to_str(&doc), "30");

    let up = XPath::compile("parent::r").expect("valid");
    assert_eq!(
        up.evaluate_from(&doc, second)
            .nodes()
            .map_or(0, <[oxml::NodeId]>::len),
        1
    );
}

#[test]
fn a_compiled_expression_exposes_its_syntax_tree() {
    // Public so callers can inspect or cache; if it stops compiling
    // the API has silently changed.
    let x = XPath::compile("//b[1]").expect("valid");
    assert!(!format!("{:?}", x.expr()).is_empty());
}

#[test]
fn a_deeply_nested_expression_is_an_error_not_a_stack_overflow() {
    // An XPath expression is untrusted input in every front end of
    // this crate: the CLI takes it from a shell, the MCP server from a
    // model, the WASM bindings from JavaScript. Recursive-descent
    // parsing without a bound turns that into a process abort no
    // caller can catch.
    assert!(XPath::compile(&"(".repeat(100)).is_err());
    for depth in [1_000usize, 100_000] {
        let expr = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
        let e = XPath::compile(&expr).expect_err("beyond the limit");
        assert!(e.to_string().contains("deep"), "{e}");
    }
}

#[test]
fn nesting_within_the_limit_still_compiles() {
    let depth = 50;
    let expr = format!("{}1 + 1{}", "(".repeat(depth), ")".repeat(depth));
    let doc = parse(DOC).expect("well-formed");
    let x = XPath::compile(&expr).expect("within the limit");
    assert_eq!(x.evaluate(&doc).to_str(&doc), "2");
}

/// `local-name()` and `namespace-uri()` describe attributes too.
///
/// Both read the node's expanded name, and `XPath` 1.0 gives one to
/// elements *and* attributes. Reading only the element name made both
/// answer the empty string for every attribute, which is wrong on its
/// own and also broke the only workaround available for selecting an
/// attribute by namespace -- so a query returned nothing and looked
/// like an empty document rather than a defect.
#[test]
fn name_functions_describe_attributes_and_pis_not_only_elements() {
    let doc = parse(r#"<r xmlns:m="urn:u"><x m:a="A" b="B"/><?pi go?></r>"#)
        .expect("well-formed");

    for (expr, want) in [
        ("local-name(//@m:a)", "a"),
        ("namespace-uri(//@m:a)", "urn:u"),
        // An attribute in no namespace has an empty namespace URI, not
        // an absent one.
        ("local-name(//@b)", "b"),
        ("namespace-uri(//@b)", ""),
        // Elements still work.
        ("local-name(//x)", "x"),
        ("namespace-uri(//x)", ""),
        // A processing instruction has a local part -- its target --
        // and no namespace.
        ("local-name(//processing-instruction())", "pi"),
        ("namespace-uri(//processing-instruction())", ""),
        // Text, comments and the root have neither.
        ("local-name(/)", ""),
    ] {
        let value = XPath::compile_with_namespaces(expr, &[("m", "urn:u")])
            .expect("compiles")
            .evaluate(&doc);
        assert_eq!(value.to_str(&doc), want, "{expr}");
    }
}

/// Selecting an attribute by its namespace with `namespace-uri()`.
///
/// Once the workaround for name tests not resolving prefixes; now the
/// way to select on a namespace without naming a prefix at all, which
/// is still useful when the URI is known and the prefix is not.
#[test]
fn attributes_can_be_selected_by_namespace_uri() {
    let doc = parse(r#"<r xmlns:m="urn:u"><x m:a="A" b="B"/></r>"#)
        .expect("well-formed");

    let namespaced = XPath::compile("//@*[namespace-uri()='urn:u']")
        .expect("compiles")
        .evaluate(&doc);
    assert_eq!(namespaced.nodes().expect("a node-set").len(), 1);
    assert_eq!(namespaced.to_str(&doc), "A");

    let plain = XPath::compile("//@*[namespace-uri()='']")
        .expect("compiles")
        .evaluate(&doc);
    assert_eq!(plain.nodes().expect("a node-set").len(), 1);
    assert_eq!(plain.to_str(&doc), "B");
}

/// A prefix in an expression resolves against the expression's own
/// bindings, not the document's declarations.
///
/// Before 0.0.4 a prefixed name test matched on its local part alone,
/// so `//m:a` selected every `a` whatever its namespace. That is a
/// wrong answer with no error attached, which is the worst way for a
/// query engine to fail: nothing distinguishes it from a document that
/// really does contain those nodes.
#[test]
fn a_prefixed_name_test_matches_only_that_namespace() {
    let doc = parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a><a>no</a></r>"#)
        .expect("well-formed");

    let bound = XPath::compile_with_namespaces("//m:a", &[("m", "urn:u")])
        .expect("compiles");
    let found = bound.evaluate(&doc);
    assert_eq!(found.nodes().expect("a node-set").len(), 1);
    assert_eq!(found.to_str(&doc), "yes");
}

#[test]
fn only_the_uri_matters_never_the_prefix() {
    // The expression may use a different prefix from the document, and
    // a document that renames its prefixes still matches. This is the
    // reason resolution happens against the expression's bindings.
    let doc =
        parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a></r>"#).expect("well-formed");
    let other = parse(r#"<r xmlns:zz="urn:u"><zz:a>yes</zz:a></r>"#)
        .expect("well-formed");

    let query = XPath::compile_with_namespaces("//q:a", &[("q", "urn:u")])
        .expect("compiles");
    assert_eq!(query.evaluate(&doc).to_str(&doc), "yes");
    assert_eq!(query.evaluate(&other).to_str(&other), "yes");
}

#[test]
fn an_unbound_prefix_is_a_compile_error() {
    // Loud, rather than a match on the local part.
    let error = XPath::compile("//m:a").expect_err("unbound");
    assert!(
        error.message.contains("unbound namespace prefix"),
        "{}",
        error.message
    );
    // Binding a different prefix does not help.
    let error = XPath::compile_with_namespaces("//m:a", &[("q", "urn:u")])
        .expect_err("still unbound");
    assert!(error.message.contains("unbound"), "{}", error.message);
}

#[test]
fn the_xml_prefix_is_bound_without_being_declared() {
    // Bound by specification, in expressions as in documents.
    let doc = parse(r#"<r><a xml:lang="en">x</a></r>"#).expect("well-formed");
    let query = XPath::compile("//@xml:lang").expect("compiles");
    assert_eq!(query.evaluate(&doc).to_str(&doc), "en");
}

#[test]
fn an_unprefixed_name_test_matches_no_namespace_only() {
    // XPath 1.0's classic surprise, and what every conforming engine
    // does: a default namespace does not apply to node tests.
    let doc = parse(r#"<r xmlns:m="urn:u"><m:a>ns</m:a><a>plain</a></r>"#)
        .expect("well-formed");
    let found = XPath::compile("//a").expect("compiles").evaluate(&doc);
    assert_eq!(found.nodes().expect("a node-set").len(), 1);
    assert_eq!(found.to_str(&doc), "plain");

    // So a document with a default namespace matches nothing by a bare
    // name -- the case that surprises people.
    let defaulted = parse(r#"<r xmlns="urn:u"><item/></r>"#).expect("ok");
    let bare = XPath::compile("//item").expect("compiles");
    assert_eq!(bare.evaluate(&defaulted).nodes().expect("set").len(), 0);

    let prefixed =
        XPath::compile_with_namespaces("//x:item", &[("x", "urn:u")])
            .expect("compiles");
    assert_eq!(prefixed.evaluate(&defaulted).nodes().expect("set").len(), 1);
}

#[test]
fn a_wildcard_still_ignores_namespaces() {
    let doc = parse(r#"<r xmlns:m="urn:u"><m:a/><a/></r>"#).expect("ok");
    let all = XPath::compile("//*").expect("compiles").evaluate(&doc);
    assert_eq!(all.nodes().expect("a node-set").len(), 3, "r, m:a, a");
}

/// A document with a DTD, so `id()` has something to match: an
/// attribute is an ID because it was *declared* one, and a document
/// without a DTD has no IDs at all.
const DTD_DOC: &str = "\
<!DOCTYPE r [<!ELEMENT r ANY><!ELEMENT a ANY><!ATTLIST a key ID #IMPLIED>]>\
<r xmlns:m=\"urn:m\">\
<a key=\"k1\" xml:lang=\"en-GB\">alpha<deep/></a>\
<b>12</b><b>30</b>\
<m:tagged>ns</m:tagged>\
</r>";

fn dtd_val(expr: &str) -> String {
    let doc = parse(DTD_DOC).expect("well-formed");
    XPath::compile_with_namespaces(expr, &[("m", "urn:m")])
        .unwrap_or_else(|e| panic!("`{expr}` failed to compile: {e}"))
        .evaluate(&doc)
        .to_str(&doc)
}

/// Every function `XPath` 1.0 defines, exercised once.
///
/// **Every expected value here is deliberately non-empty.** An
/// unimplemented function falls through the evaluator's final arm to an
/// empty node-set, whose string-value is `""` — so a table with an
/// empty expectation anywhere would pass on a function that does not
/// exist. That is exactly how six of these went missing without a
/// single test failing: they compiled, returned `""`, and `""` was
/// indistinguishable from a document that genuinely had no match.
#[test]
fn every_xpath_1_0_function_is_implemented() {
    let cases: &[(&str, &str, &str)] = &[
        // (function, expression, expected)
        ("last", "string(//b[last()])", "30"),
        ("position", "string(//b[position()=1])", "12"),
        ("count", "count(//b)", "2"),
        ("id", "string(id('k1'))", "alpha"),
        ("local-name", "local-name(//m:tagged)", "tagged"),
        ("namespace-uri", "namespace-uri(//m:tagged)", "urn:m"),
        ("name", "name(//m:tagged)", "m:tagged"),
        ("string", "string(//b)", "12"),
        ("concat", "concat('a','b')", "ab"),
        ("starts-with", "string(starts-with('abc','a'))", "true"),
        ("contains", "string(contains('abc','b'))", "true"),
        ("substring-before", "substring-before('a/b','/')", "a"),
        ("substring-after", "substring-after('a/b','/')", "b"),
        ("substring", "substring('12345',2,3)", "234"),
        ("string-length", "string-length('abcd')", "4"),
        ("normalize-space", "normalize-space('  a  b ')", "a b"),
        ("translate", "translate('bar','abc','ABC')", "BAr"),
        ("boolean", "string(boolean(1))", "true"),
        ("not", "string(not(0))", "true"),
        ("true", "string(true())", "true"),
        ("false", "string(false())", "false"),
        ("lang", "string(//a[lang('en')]/@key)", "k1"),
        ("number", "number('42')", "42"),
        ("sum", "sum(//b)", "42"),
        ("floor", "floor(1.9)", "1"),
        ("ceiling", "ceiling(1.1)", "2"),
        ("round", "round(-1.5)", "-1"),
    ];
    assert_eq!(cases.len(), 27, "XPath 1.0 defines 27 functions");
    for (name, expr, want) in cases {
        assert!(!want.is_empty(), "{name}: expectation must be non-empty");
        assert_eq!(dtd_val(expr), *want, "{name}: `{expr}`");
    }
}

/// A name the library does not implement must fail to *compile*.
///
/// It used to compile and evaluate to an empty node-set, which meant a
/// typo returned "no matches" instead of an error — the caller could
/// not tell a misspelled function from an absent element.
#[test]
fn unknown_function_is_a_compile_error() {
    for expr in [
        "frobnicate()",
        "not-a-function('x')",
        "substring-bfore('a/b','/')",
        "ends-with('abc','c')", // XPath 2.0, not 1.0
        "lower-case('A')",      // XPath 2.0, not 1.0
        "//a[frobnicate()]",
    ] {
        let err = XPath::compile(expr)
            .expect_err("unknown function must not compile");
        assert!(
            err.message.contains("unknown function"),
            "`{expr}` reported {err}"
        );
    }
}

/// Node tests share the `name(` shape with function calls and must not
/// be caught by the unknown-function check.
#[test]
fn node_tests_still_compile() {
    for expr in [
        "//text()",
        "//comment()",
        "//node()",
        "//processing-instruction()",
        "//processing-instruction('target')",
    ] {
        assert!(XPath::compile(expr).is_ok(), "`{expr}` must compile");
    }
}

#[test]
fn substring_before_and_after_on_no_match_are_empty() {
    // Not an error, and not the whole string: the specification says
    // the empty string when the substring does not occur.
    assert_eq!(dtd_val("substring-before('abc','z')"), "");
    assert_eq!(dtd_val("substring-after('abc','z')"), "");
    // The specification's own examples.
    assert_eq!(dtd_val("substring-before('1999/04/01','/')"), "1999");
    assert_eq!(dtd_val("substring-after('1999/04/01','/')"), "04/01");
}

#[test]
fn translate_removes_characters_with_no_replacement() {
    // The specification's own examples. A search character with no
    // corresponding replacement character is removed, not passed
    // through — `-` here has no partner.
    assert_eq!(dtd_val("translate('bar','abc','ABC')"), "BAr");
    assert_eq!(dtd_val("translate('--aaa--','abc-','ABC')"), "AAA");
    // A character repeated in the search string takes its *first*
    // replacement, so `a` maps to `X` and never to `Y`.
    assert_eq!(dtd_val("translate('abc','aab','XYZ')"), "XZc");
}

#[test]
fn lang_is_inherited_and_case_insensitive() {
    // `deep` carries no `xml:lang` of its own and must inherit `a`'s.
    assert_eq!(dtd_val("string(//deep[lang('en')])"), "");
    assert_eq!(dtd_val("count(//deep[lang('en')])"), "1");
    // A subtag matches the bare tag, and case is not significant.
    assert_eq!(dtd_val("count(//a[lang('en')])"), "1");
    assert_eq!(dtd_val("count(//a[lang('EN')])"), "1");
    assert_eq!(dtd_val("count(//a[lang('en-gb')])"), "1");
    // A prefix that is not a whole subtag does not match, and an
    // element above the declaration has no language in scope.
    assert_eq!(dtd_val("count(//a[lang('e')])"), "0");
    assert_eq!(dtd_val("count(//r[lang('en')])"), "0");
}

#[test]
fn name_reports_the_prefix_and_local_name_does_not() {
    assert_eq!(dtd_val("name(//m:tagged)"), "m:tagged");
    assert_eq!(dtd_val("local-name(//m:tagged)"), "tagged");
    // An unprefixed name has no colon to report.
    assert_eq!(dtd_val("name(//b)"), "b");
    assert_eq!(dtd_val("local-name(//b)"), "b");
    // Attributes have expanded names too.
    assert_eq!(dtd_val("name(//a/@key)"), "key");
    // Nothing without an expanded name answers the empty string.
    assert_eq!(dtd_val("name(/)"), "");
}

/// Two prefixes bound to one namespace name the same thing, so they
/// share an interned name — but `name()` has to report the prefix each
/// was *written* with, not whichever was seen first.
#[test]
fn name_distinguishes_two_prefixes_for_one_namespace() {
    let doc = parse("<r xmlns:x='urn:u' xmlns:y='urn:u'><x:a/><y:a/></r>")
        .expect("well-formed");
    let name_of = |expr: &str| {
        XPath::compile(expr)
            .expect("compiles")
            .evaluate(&doc)
            .to_str(&doc)
    };
    // Both elements expand to the same name and share one interned
    // entry, so a single prefix per entry would report one of them
    // wrongly.
    let first = name_of("name(//*[local-name()='a'][1])");
    let second = name_of("name(//*[local-name()='a'][2])");
    assert_eq!((first.as_str(), second.as_str()), ("x:a", "y:a"));
}

#[test]
fn id_matches_declared_id_attributes_only() {
    // `k1` is declared ID; a document without a DTD declares none, so
    // `id()` there is empty however the attribute is spelled.
    assert_eq!(dtd_val("string(id('k1'))"), "alpha");
    assert_eq!(dtd_val("count(id('absent'))"), "0");

    let plain = parse("<r><a id='k1'>alpha</a></r>").expect("well-formed");
    let n = XPath::compile("count(id('k1'))")
        .expect("compiles")
        .evaluate(&plain)
        .to_str(&plain);
    assert_eq!(n, "0", "no DTD means no ID-typed attributes");
}

#[test]
fn id_takes_a_whitespace_separated_list_and_dedups() {
    let src = "\
<!DOCTYPE r [<!ELEMENT r ANY><!ELEMENT a ANY><!ATTLIST a key ID #IMPLIED>]>\
<r><a key=\"p\">1</a><a key=\"q\">2</a></r>";
    let doc = parse(src).expect("well-formed");
    let count = |expr: &str| {
        XPath::compile(expr)
            .expect("compiles")
            .evaluate(&doc)
            .to_str(&doc)
    };
    assert_eq!(count("count(id('p q'))"), "2");
    // A node-set is a set: the same ID twice selects one node.
    assert_eq!(count("count(id('p p'))"), "1");
    assert_eq!(count("count(id('p absent'))"), "1");
    // Results come back in document order regardless of argument order.
    assert_eq!(count("string(id('q p'))"), "1");
}

/// The wrong number of arguments is a compile error.
///
/// These used to compile and answer something plausible, which is worse
/// than answering nothing: `starts-with('abc')` was **true**, because
/// the absent second argument read as the empty string and every string
/// starts with that. `translate('abc','ab')` deleted the characters it
/// had no replacement for and returned `"c"`.
#[test]
fn the_wrong_number_of_arguments_is_a_compile_error() {
    for expr in [
        "starts-with('abc')",     // needs 2
        "substring-before('a')",  // needs 2
        "substring-after('a')",   // needs 2
        "translate('abc','ab')",  // needs 3
        "contains('abc')",        // needs 2
        "concat('a')",            // needs 2 or more
        "id()",                   // needs 1
        "not()",                  // needs 1
        "floor()",                // needs 1
        "sum()",                  // needs 1
        "lang()",                 // needs 1
        "substring('abc')",       // needs 2 or 3
        "true(1)",                // takes none
        "false(1)",               // takes none
        "position(1)",            // takes none
        "last(1)",                // takes none
        "string-length('a','b')", // takes at most 1
        "name('a','b')",          // takes at most 1
        "substring('a',1,2,3)",   // takes at most 3
    ] {
        let err =
            XPath::compile(expr).expect_err("wrong arity must not compile");
        assert!(
            err.message.contains("wrong number of arguments"),
            "`{expr}` reported {err}"
        );
    }
}

/// The arities that *are* correct must still compile, including the
/// optional-argument and variadic forms.
#[test]
fn the_specified_arities_compile() {
    for expr in [
        "last()",
        "position()",
        "count(//a)",
        "id('k')",
        "local-name()",
        "local-name(//a)",
        "namespace-uri()",
        "namespace-uri(//a)",
        "name()",
        "name(//a)",
        "string()",
        "string(//a)",
        "concat('a','b')",
        "concat('a','b','c','d','e')", // variadic
        "substring('abc',2)",
        "substring('abc',2,1)",
        "string-length()",
        "string-length('a')",
        "normalize-space()",
        "normalize-space('a')",
        "translate('a','b','c')",
        "lang('en')",
        "number()",
        "number('1')",
        "round(1.5)",
    ] {
        assert!(XPath::compile(expr).is_ok(), "`{expr}` must compile");
    }
}

/// `id()` accepts a node-set, in which case each node's string-value is
/// itself a whitespace-separated list of IDs.
#[test]
fn id_accepts_a_node_set_argument() {
    let src = "\
<!DOCTYPE r [<!ELEMENT r ANY><!ELEMENT a ANY><!ELEMENT ref ANY>\
<!ATTLIST a key ID #IMPLIED>]>\
<r>\
<a key=\"p\">1</a><a key=\"q\">2</a><a key=\"s\">3</a>\
<ref>p</ref><ref>q s</ref>\
</r>";
    let doc = parse(src).expect("well-formed");
    let val = |expr: &str| {
        XPath::compile(expr)
            .expect("compiles")
            .evaluate(&doc)
            .to_str(&doc)
    };
    // One node, one ID.
    assert_eq!(val("count(id(//ref[1]))"), "1");
    // One node whose string-value lists two IDs.
    assert_eq!(val("count(id(//ref[2]))"), "2");
    // Both nodes, three IDs between them.
    assert_eq!(val("count(id(//ref))"), "3");
    // A node-set selecting nothing selects no elements either.
    assert_eq!(val("count(id(//absent))"), "0");
}

/// A processing instruction has a name -- its target -- and no prefix.
#[test]
fn name_of_a_processing_instruction_is_its_target() {
    let doc = parse("<r><?render mode='fast'?><a/></r>").expect("well-formed");
    let val = |expr: &str| {
        XPath::compile(expr)
            .expect("compiles")
            .evaluate(&doc)
            .to_str(&doc)
    };
    assert_eq!(val("name(//processing-instruction())"), "render");
    assert_eq!(val("local-name(//processing-instruction())"), "render");
    // A target is not in a namespace, however it is spelled.
    assert_eq!(val("namespace-uri(//processing-instruction())"), "");
    // Comments and text have no name at all.
    assert_eq!(val("name(//text())"), "");
}
