// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Number-to-string conversion, which `XPath` specifies precisely.

#![cfg(feature = "xpath")]

use oxml::{XPath, parse};

fn s(expr: &str) -> String {
    let d = parse("<r/>").expect("parses");
    XPath::compile(expr)
        .expect("compiles")
        .evaluate(&d)
        .to_str(&d)
}

#[test]
fn integers_print_without_a_decimal_point() {
    assert_eq!(s("1"), "1");
    assert_eq!(s("0"), "0");
    assert_eq!(s("-4"), "-4");
    assert_eq!(s("2 * 3"), "6");
}

#[test]
fn fractions_keep_their_digits() {
    assert_eq!(s("1.5"), "1.5");
    assert_eq!(s("0.25"), "0.25");
}

/// The case that motivated 15-significant-digit rounding.
///
/// `9.99 + 7.50` is not exactly 17.49 in IEEE 754. Printing every
/// digit needed to distinguish the value gives `17.490000000000002`;
/// every other `XPath` engine prints `17.49`.
#[test]
fn accumulated_float_noise_is_trimmed() {
    assert_eq!(s("9.99 + 7.50"), "17.49");
    assert_eq!(s("0.1 + 0.2"), "0.3");
}

#[test]
fn division_that_does_not_terminate_still_prints_sensibly() {
    // 1 div 3 has no exact representation; 15 significant digits is
    // the agreed cut-off.
    let v = s("1 div 3");
    assert!(v.starts_with("0.333333333333333"), "got {v}");
}

#[test]
fn special_values_use_their_xpath_names() {
    assert_eq!(s("number('abc')"), "NaN");
    assert_eq!(s("1 div 0"), "Infinity");
    assert_eq!(s("-1 div 0"), "-Infinity");
}

#[test]
fn booleans_print_as_words() {
    assert_eq!(s("true()"), "true");
    assert_eq!(s("false()"), "false");
}
