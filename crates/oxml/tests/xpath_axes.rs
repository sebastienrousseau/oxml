// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Axes, node tests and operators.
//!
//! `XPath` 1.0 defines each axis as a specific node sequence; an axis
//! that returns *some* plausible nodes is not the same as one that
//! returns the right ones, so each is pinned against a document whose
//! shape makes the difference visible.

use oxml::{XPath, parse};

const DOC: &str = "\
<library count=\"3\">\
<!-- catalogue -->\
<book id=\"1\" lang=\"en\"><title>Dune</title><year>1965</year></book>\
<book id=\"2\" lang=\"fr\"><title>Germinal</title><year>1885</year></book>\
<book id=\"3\" lang=\"en\"><title>Neuromancer</title><year>1984</year></book>\
<?render mode=\"list\"?>\
</library>";

/// Evaluate `expr` and return its string value.
fn val(expr: &str) -> String {
    let doc = parse(DOC).expect("well-formed");
    XPath::compile(expr)
        .unwrap_or_else(|e| panic!("`{expr}` failed to compile: {e}"))
        .evaluate(&doc)
        .to_str(&doc)
}

/// Evaluate `expr` and return the number of nodes it selects.
fn count(expr: &str) -> usize {
    let doc = parse(DOC).expect("well-formed");
    XPath::compile(expr)
        .unwrap_or_else(|e| panic!("`{expr}` failed to compile: {e}"))
        .evaluate(&doc)
        .nodes()
        .map_or(0, <[oxml::NodeId]>::len)
}

/// The text of every node `expr` selects.
fn texts(expr: &str) -> Vec<String> {
    let doc = parse(DOC).expect("well-formed");
    let value = XPath::compile(expr).expect("valid").evaluate(&doc);
    value
        .nodes()
        .map(|n| n.iter().map(|id| doc.text(*id)).collect())
        .unwrap_or_default()
}

#[test]
fn the_descendant_axis_excludes_the_context_node() {
    // The difference from descendant-or-self is exactly one node, and
    // it is the one most easily got wrong.
    assert_eq!(count("/library/descendant::book"), 3);
    assert_eq!(count("/library/descendant::library"), 0);
    assert_eq!(count("/library/descendant-or-self::library"), 1);
}

#[test]
fn the_ancestor_axes_walk_to_the_root() {
    assert_eq!(count("//title/ancestor::book"), 3);
    assert_eq!(count("//title[1]/ancestor::*"), 2, "book and library");
    assert_eq!(
        count("//title[1]/ancestor-or-self::*"),
        3,
        "title, book and library"
    );
    assert_eq!(count("//title[1]/ancestor::title"), 0);
}

#[test]
fn sibling_axes_exclude_the_context_node_and_respect_direction() {
    assert_eq!(
        texts("//book[1]/following-sibling::book/title"),
        ["Germinal", "Neuromancer"]
    );
    assert_eq!(
        texts("//book[3]/preceding-sibling::book/title"),
        ["Dune", "Germinal"]
    );
    assert_eq!(count("//book[1]/preceding-sibling::book"), 0);
    assert_eq!(count("//book[3]/following-sibling::book"), 0);
    assert_eq!(
        count("//book[2]/following-sibling::book"),
        1,
        "the context node is not its own sibling"
    );
}

#[test]
fn the_sibling_axes_of_the_root_are_empty_rather_than_a_panic() {
    assert_eq!(count("/library/following-sibling::*"), 0);
    assert_eq!(count("/library/preceding-sibling::*"), 0);
}

#[test]
fn the_parent_and_self_axes_select_one_node_or_none() {
    assert_eq!(val("//title[1]/parent::book/@id"), "1");
    assert_eq!(count("//title[1]/self::title"), 1);
    assert_eq!(count("//title[1]/self::book"), 0);
    assert_eq!(count("/library/parent::*"), 0, "the root has no parent");
}

#[test]
fn the_attribute_axis_selects_attributes_not_their_element() {
    assert_eq!(val("//book[1]/@lang"), "en");
    assert_eq!(count("//book/@lang"), 3);
    assert_eq!(count("//book/@*"), 6, "id and lang on each of three");
    assert_eq!(count("//book/@missing"), 0);
}

#[test]
fn node_tests_select_by_kind() {
    assert_eq!(count("//comment()"), 1);
    assert_eq!(count("//processing-instruction()"), 1);
    assert!(count("//text()") > 0);
    assert!(count("//node()") > count("//*"), "node() is wider");
}

#[test]
fn a_comment_and_a_processing_instruction_have_string_values() {
    assert!(val("//comment()").contains("catalogue"));
    assert!(val("//processing-instruction()").contains("list"));
}

#[test]
fn the_wildcard_selects_elements_only() {
    // Not attributes, comments or text, which is what distinguishes
    // `*` from `node()`.
    assert_eq!(count("/library/*"), 3);
}

#[test]
fn every_comparison_operator_is_implemented() {
    assert_eq!(val("2 < 3"), "true");
    assert_eq!(val("3 < 3"), "false");
    assert_eq!(val("3 <= 3"), "true");
    assert_eq!(val("4 > 3"), "true");
    assert_eq!(val("3 >= 4"), "false");
    assert_eq!(val("3 = 3"), "true");
    assert_eq!(val("3 != 3"), "false");
}

#[test]
fn every_arithmetic_operator_is_implemented() {
    assert_eq!(val("2 + 3"), "5");
    assert_eq!(val("5 - 2"), "3");
    assert_eq!(val("3 * 4"), "12");
    assert_eq!(val("7 div 2"), "3.5");
    assert_eq!(val("7 mod 2"), "1");
    assert_eq!(val("-3 + 1"), "-2");
}

#[test]
fn boolean_operators_combine_conditions() {
    assert_eq!(val("true() and true()"), "true");
    assert_eq!(val("true() and false()"), "false");
    assert_eq!(val("false() or true()"), "true");
    assert_eq!(val("false() or false()"), "false");
    assert_eq!(val("not(false())"), "true");
}

#[test]
fn a_node_set_compares_existentially() {
    // XPath 1.0: the comparison holds if *any* node satisfies it. This
    // is the rule most often implemented as "the first node".
    assert_eq!(val("//book/@lang = 'fr'"), "true");
    assert_eq!(val("//book/@lang = 'de'"), "false");
    assert_eq!(val("//year > 1900"), "true", "at least one year is later");
    assert_eq!(val("//year < 1000"), "false");
}

#[test]
fn two_node_sets_compare_by_any_pair() {
    assert_eq!(val("//book/@id = //book/@id"), "true");
    assert_eq!(val("//title = //year"), "false");
}

#[test]
fn a_node_set_compares_against_a_number_by_conversion() {
    assert_eq!(val("//year = 1965"), "true");
    assert_eq!(val("//year = 2000"), "false");
}

#[test]
fn a_predicate_can_filter_on_an_attribute_value() {
    assert_eq!(texts("//book[@lang='en']/title"), ["Dune", "Neuromancer"]);
    assert_eq!(count("//book[@lang='de']"), 0);
}

#[test]
fn position_and_last_are_one_based() {
    assert_eq!(texts("//book[position()=1]/title"), ["Dune"]);
    assert_eq!(texts("//book[last()]/title"), ["Neuromancer"]);
    assert_eq!(val("count(//book[position() > 1])"), "2");
}

#[test]
fn a_boolean_value_converts_to_a_number() {
    assert_eq!(val("true() + 1"), "2");
    assert_eq!(val("false() + 1"), "1");
}

#[test]
fn an_empty_node_set_is_false_and_a_non_empty_one_is_true() {
    assert_eq!(val("boolean(//book)"), "true");
    assert_eq!(val("boolean(//missing)"), "false");
}

#[test]
fn nan_is_false_and_a_non_zero_number_is_true() {
    assert_eq!(val("boolean(number('x'))"), "false", "NaN is false");
    assert_eq!(val("boolean(0)"), "false");
    assert_eq!(val("boolean(1)"), "true");
    assert_eq!(val("boolean(-1)"), "true");
}

#[test]
fn an_empty_string_is_false_and_any_other_string_is_true() {
    assert_eq!(val("boolean('')"), "false");
    assert_eq!(val("boolean('a')"), "true");
    assert_eq!(val("boolean('0')"), "true", "a non-empty string is true");
}

#[test]
fn a_processing_instruction_can_be_narrowed_to_its_target() {
    // XPath 1.0 gives this node test an optional literal argument; it
    // is the only one that takes one.
    assert_eq!(count("//processing-instruction('render')"), 1);
    assert_eq!(count("//processing-instruction('other')"), 0);
    assert_eq!(count(r#"//processing-instruction("render")"#), 1);
}

#[test]
fn an_unknown_node_test_is_still_rejected() {
    assert!(XPath::compile("//madeup()").is_err());
    // A malformed processing-instruction test must not be accepted
    // just because the name matched.
    assert!(XPath::compile("//processing-instruction(").is_err());
    assert!(XPath::compile("//processing-instruction('x'").is_err());
}

#[test]
fn abbreviated_syntax_expands_to_the_full_form() {
    // `.` is self::node(), `..` is parent::node(), `//` inserts a
    // descendant-or-self step. Each is a distinct code path from the
    // spelled-out axis.
    assert_eq!(count("//book[1]/./title"), 1);
    assert_eq!(count("//title[1]/../.."), 1, ".. twice reaches library");
    assert_eq!(count("//book[1]/..//title"), 3, "// after ..");
    assert_eq!(count(".//book"), 3, "a relative descendant path");
}

#[test]
fn an_absolute_path_starts_from_the_document() {
    assert_eq!(count("/library"), 1);
    assert_eq!(count("/library/book"), 3);
    assert_eq!(count("/book"), 0, "book is not the document element");
    assert_eq!(count("/"), 1, "the document node itself");
}

#[test]
fn the_union_operator_merges_node_sets() {
    assert_eq!(count("//title | //year"), 6);
    assert_eq!(count("//title | //title"), 3, "a union deduplicates");
    assert_eq!(count("//title | //missing"), 3);
}

#[test]
fn a_parenthesised_expression_can_be_a_whole_path() {
    assert_eq!(val("(1 + 2) * 3"), "9");
    assert_eq!(val("count((//title | //year))"), "6");
}

#[test]
fn an_unclosed_parenthesis_or_predicate_is_rejected() {
    for bad in [
        "(1 + 2",
        "count(//a",
        "//book[1",
        "//book[",
        "//book]",
        "1 +",
        "| //a",
        "//",
    ] {
        assert!(XPath::compile(bad).is_err(), "`{bad}` should not compile");
    }
}

#[test]
fn trailing_input_after_a_complete_expression_is_rejected() {
    assert!(XPath::compile("1 + 1 garbage").is_err());
    assert!(XPath::compile("//book )").is_err());
}

#[test]
fn every_axis_is_reachable_by_its_full_name() {
    for axis in [
        "child",
        "descendant",
        "descendant-or-self",
        "parent",
        "ancestor",
        "ancestor-or-self",
        "self",
        "attribute",
        "following-sibling",
        "preceding-sibling",
    ] {
        let expr = format!("//book/{axis}::*");
        assert!(XPath::compile(&expr).is_ok(), "`{expr}` failed to compile");
    }
    assert!(XPath::compile("//book/nosuchaxis::*").is_err());
}

/// `following::` is everything after the context node in document
/// order that is not beneath it.
///
/// The distinction from `following-sibling::` is what makes this axis
/// worth having: from the second book, `following-sibling::` reaches
/// the third book but not its `title`, while `following::` reaches
/// both.
#[test]
fn the_following_axis_leaves_the_parent_behind() {
    // Book 2's following: book 3 and everything inside it, plus the
    // processing instruction. Not book 2's own title or year, which
    // are descendants, and not book 1, which is before it.
    assert_eq!(texts("//book[2]/following::title"), ["Neuromancer"]);
    assert_eq!(texts("//book[2]/following::year"), ["1984"]);
    assert_eq!(count("//book[2]/following::book"), 1);
    // A descendant is excluded even though it is later in the arena.
    assert_eq!(count("//book[2]/following::*[.='Germinal']"), 0);
    // From the first title, every later element in the document.
    assert_eq!(count("//book[1]/title/following::title"), 2);
    // The last node in the document has nothing following it.
    assert_eq!(count("//book[3]/year/following::*"), 0);
    // Nothing follows the root element: everything else is inside it.
    assert_eq!(count("/library/following::*"), 0);
}

/// `preceding::` is the mirror, and excludes ancestors rather than
/// descendants.
#[test]
fn the_preceding_axis_leaves_the_ancestors_behind() {
    assert_eq!(texts("//book[2]/preceding::title"), ["Dune"]);
    assert_eq!(count("//book[2]/preceding::book"), 1);
    // `library` is an ancestor of book 2, so it is not preceding it,
    // even though it opens earlier in the document.
    assert_eq!(count("//book[2]/preceding::library"), 0);
    // Nor is `book[3]`'s own parent chain.
    assert_eq!(count("//book[3]/title/preceding::book"), 2);
    // The first element has nothing before it but its ancestors.
    assert_eq!(count("//book[1]/preceding::*"), 0);
    // A comment is a node and does precede the first book.
    assert_eq!(count("//book[1]/preceding::comment()"), 1);
}

/// Attribute nodes are on neither axis, however they are positioned.
///
/// They sit between their element and its children in document order,
/// so an index comparison alone would sweep them in.
#[test]
fn attributes_are_on_neither_following_nor_preceding() {
    // Before `book[2]`, excluding its ancestors: the comment,
    // `book[1]`, its `title` and `year`, and their two text nodes --
    // six. Were attributes included, `library/@count` and `book[1]`'s
    // `@id` and `@lang` would make it nine.
    assert_eq!(count("//book[2]/preceding::node()"), 6);
    // After it, excluding its descendants: `book[3]`, its `title` and
    // `year`, their two text nodes, and the processing instruction --
    // six again. `book[3]`'s two attributes would make it eight.
    assert_eq!(count("//book[2]/following::node()"), 6);
    // The attribute axis still reaches them, of course.
    assert_eq!(count("//book[2]/attribute::*"), 2);
}

/// Both axes are reachable by their full names only; there is no
/// abbreviation for either.
#[test]
fn following_and_preceding_have_no_abbreviation() {
    assert_eq!(count("//book[2]/following::book"), 1);
    assert_eq!(count("//book[2]/preceding::book"), 1);
    // `//` is `descendant-or-self`, not `following`.
    assert_eq!(count("//book"), 3);
}
