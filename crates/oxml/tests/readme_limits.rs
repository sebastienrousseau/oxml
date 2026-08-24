// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The README's table of limit profiles must match the code.
//!
//! Documentation that restates a constant drifts from it silently, and
//! a table of plausible numbers is worse than no table because it is
//! believed. Seven of this table's ten rows were wrong when it was
//! first written; this test is why they are not now.

use oxml::Limits;

/// One row of the table: the field name and its three values.
fn table() -> Vec<(String, [String; 3])> {
    let readme = include_str!("../README.md");
    let start = readme
        .find("| Field | `strict()` | `default()` | `permissive()` |")
        .expect("the profile table is in the README");
    let mut rows = Vec::new();
    for line in readme[start..].lines().skip(2) {
        if !line.starts_with("| `") {
            break;
        }
        let cells: Vec<_> = line
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().trim_matches('`').to_string())
            .collect();
        assert_eq!(cells.len(), 4, "malformed row: {line}");
        rows.push((
            cells[0].clone(),
            [cells[1].clone(), cells[2].clone(), cells[3].clone()],
        ));
    }
    rows
}

/// The value of one field of one profile, as the table would write it.
fn actual(limits: &Limits, field: &str) -> String {
    let unbounded = |v: Option<usize>| {
        v.map_or_else(|| "unbounded".to_string(), |n| n.to_string())
    };
    match field {
        "max_depth" => limits.max_depth.to_string(),
        "max_attributes_per_element" => {
            limits.max_attributes_per_element.to_string()
        }
        "max_attribute_size" => limits.max_attribute_size.to_string(),
        "max_name_length" => limits.max_name_length.to_string(),
        "max_nodes" => unbounded(limits.max_nodes),
        "max_text_length" => unbounded(limits.max_text_length),
        "max_entity_depth" => limits.max_entity_depth.to_string(),
        "max_entity_expansion" => limits.max_entity_expansion.to_string(),
        "max_xpath_depth" => limits.max_xpath_depth.to_string(),
        "max_xpath_operators" => limits.max_xpath_operators.to_string(),
        other => panic!(
            "the README documents `{other}`, which this test does not know about -- add it here"
        ),
    }
}

#[test]
fn the_readme_table_matches_the_profiles() {
    let profiles = [
        ("strict", Limits::strict()),
        ("default", Limits::default()),
        ("permissive", Limits::permissive()),
    ];
    let rows = table();
    assert!(
        !rows.is_empty(),
        "found no rows -- the check is not working"
    );

    for (field, documented) in &rows {
        for (i, (name, limits)) in profiles.iter().enumerate() {
            assert_eq!(
                &actual(limits, field),
                &documented[i],
                "README says {name}().{field} is {}, code says {}",
                documented[i],
                actual(limits, field),
            );
        }
    }
    println!("{} documented fields match", rows.len());
}

#[test]
fn the_table_documents_every_field() {
    // A field added to `Limits` and not to the table would otherwise
    // go unmentioned, which is how the table fell behind before.
    let documented: Vec<_> = table().into_iter().map(|(f, _)| f).collect();
    let source = include_str!("../src/limits.rs");
    let declared: Vec<_> = source
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub "))
        .filter(|l| l.starts_with("max_"))
        .filter_map(|l| l.split(':').next())
        .map(str::to_string)
        .collect();
    assert!(
        !declared.is_empty(),
        "found no fields -- the check is not working"
    );

    for field in &declared {
        assert!(
            documented.contains(field),
            "`{field}` is a public limit the README's table does not document"
        );
    }
}

/// The README's list of `XPath` functions must be the functions that
/// exist.
///
/// The list previously announced "25 functions" and then named 21, of
/// which the library implemented 21 — three different numbers, none
/// checked against another. Six functions the specification requires
/// were absent from all three and from the tests, and compiled to an
/// empty result rather than an error, so nothing failed.
#[test]
fn the_readme_lists_every_xpath_function_and_no_others() {
    let readme = include_str!("../README.md");
    let start = readme
        .find("- All 27 functions: ")
        .expect("the function list is in the README");
    let bullet: String = readme[start..]
        .lines()
        .take_while(|l| !l.starts_with("- Arithmetic"))
        .collect::<Vec<_>>()
        .join(" ");

    let listed: Vec<&str> = bullet.split('`').skip(1).step_by(2).collect();

    assert_eq!(
        listed.len(),
        27,
        "the README names {} functions but says 27: {listed:?}",
        listed.len()
    );

    // Every name listed must compile as a call at *some* arity, and
    // anything not listed at none. Trying each arity rather than
    // asserting a particular one keeps the specified argument counts in
    // one place -- the parser's table -- instead of restating them here
    // where they could drift.
    for name in &listed {
        assert!(
            compiles_at_some_arity(name),
            "README lists `{name}` but no call to it compiles"
        );
    }
    assert!(
        !compiles_at_some_arity("definitely-not-a-function"),
        "an unlisted name must not compile at any arity"
    );
}

/// Whether a call to `name` compiles with anywhere from zero to three
/// arguments.
fn compiles_at_some_arity(name: &str) -> bool {
    (0..=3).any(|n| {
        let args = ["'a'"; 3][..n].join(",");
        oxml::XPath::compile(&format!("{name}({args})")).is_ok()
    })
}
