// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Shared XML character and name rules, independent of parser and tree.

/// The XML version a document declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Version {
    /// XML 1.0, the default when no declaration says otherwise.
    V10,
    /// XML 1.1.
    V11,
}

/// `Char`, for the version in force — the rule for a character
/// arriving via a **reference**.
///
/// XML 1.1 widens the production to admit the C0 and C1 controls.
#[must_use]
pub(crate) const fn is_xml_char_for(c: char, version: Version) -> bool {
    match version {
        Version::V10 => is_xml_char(c),
        Version::V11 => matches!(c,
            '\u{1}'..='\u{D7FF}'
            | '\u{E000}'..='\u{FFFD}'
            | '\u{10000}'..='\u{10FFFF}'
        ),
    }
}

/// `RestrictedChar`, XML 1.1 production 2a.
///
/// ```text
/// RestrictedChar ::= [#x1-#x8] | [#xB-#xC] | [#xE-#x1F]
///                  | [#x7F-#x84] | [#x86-#x9F]
/// ```
#[must_use]
const fn is_restricted_char(c: char) -> bool {
    matches!(c,
        '\u{1}'..='\u{8}' | '\u{B}'..='\u{C}' | '\u{E}'..='\u{1F}'
        | '\u{7F}'..='\u{84}' | '\u{86}'..='\u{9F}'
    )
}

/// Whether `c` may appear **literally** in the document text.
///
/// This is deliberately stricter than [`is_xml_char_for`]. XML 1.1
/// admits the C0 and C1 controls into `Char`, but production 2a
/// forbids them appearing literally: they must be written as character
/// references. Conflating the two rules accepts 63 documents the suite
/// marks not-well-formed — a control character pasted into a comment or
/// into content is exactly the case this separates.
#[must_use]
pub(crate) const fn is_literal_char_for(c: char, version: Version) -> bool {
    match version {
        Version::V10 => is_xml_char(c),
        Version::V11 => is_xml_char_for(c, version) && !is_restricted_char(c),
    }
}

/// Whether `c` matches XML 1.0 production 2, `Char`.
///
/// ```text
/// Char ::= #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD]
///        | [#x10000-#x10FFFF]
/// ```
///
/// Most C0 control characters are **not** legal anywhere in an XML
/// document — not in content, not in an attribute value, not even
/// inside a comment. A parser that accepts them is accepting documents
/// that are not well-formed, and this accounted for 77 failures against
/// the W3C suite.
///
/// Surrogates cannot appear: Rust's `char` cannot hold one. `#xFFFE`
/// and `#xFFFF` can, and are excluded.
#[must_use]
pub(crate) const fn is_xml_char(c: char) -> bool {
    matches!(c,
        '\u{9}' | '\u{A}' | '\u{D}'
        | '\u{20}'..='\u{D7FF}'
        | '\u{E000}'..='\u{FFFD}'
        | '\u{10000}'..='\u{10FFFF}'
    )
}

pub(crate) fn is_name_start(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z' | '_' | ':')
        || matches!(c as u32,
            0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF
            | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D
            | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF
            | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

pub(crate) fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9')
        || c as u32 == 0xB7
        || matches!(c as u32, 0x300..=0x36F | 0x203F..=0x2040)
}
