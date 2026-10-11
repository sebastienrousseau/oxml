// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The lexical layer of the `XPath` parser: whitespace, tokens, names
//! and string literals.

use alloc::borrow::ToOwned;
use alloc::string::String;

use super::P;

impl P<'_> {
    pub(super) fn ws(&mut self) {
        while self.i < self.b.len()
            && matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n')
        {
            self.i += 1;
        }
    }

    pub(super) fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(tok) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    /// Match a word operator, ensuring it is not the prefix of a name.
    ///
    /// Without the boundary check, `andover` would lex as `and` +
    /// `over`, and a path step named `divide` would become a division.
    pub(super) fn eat_word(&mut self, w: &str) -> bool {
        self.ws();
        let rest = &self.s[self.i..];
        if let Some(after) = rest.strip_prefix(w) {
            if after.chars().next().is_none_or(|c| !is_name_char(c)) {
                self.i += w.len();
                return true;
            }
        }
        false
    }

    pub(super) fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.b.get(self.i).copied()
    }

    /// A quoted string, if one is next, leaving the input where it
    /// was otherwise -- including when the quote is never closed. Used
    /// for every string literal and for
    /// `processing-instruction('target')`, the only node test that
    /// takes an argument.
    pub(super) fn try_literal(&mut self) -> Option<String> {
        let quote = *self.b.get(self.i)?;
        if !matches!(quote, b'\'' | b'"') {
            return None;
        }
        let body = &self.s[self.i + 1..];
        let end = body.find(char::from(quote))?;
        self.i += end + 2;
        Some(body[..end].to_owned())
    }

    /// A `QName`, if one is next; the input is left where it was
    /// otherwise.
    pub(super) fn try_name(&mut self) -> Option<String> {
        self.ws();
        let rest = &self.s[self.i..];
        let mut end = 0;
        for (idx, c) in rest.char_indices() {
            if idx == 0 {
                if !is_name_start(c) {
                    return None;
                }
            } else if !is_name_char(c) {
                break;
            }
            // A `:` that is doubled is an axis separator, not part of
            // the name. Without this the name swallows `parent::a`
            // whole, the `::` test then fails, and the step silently
            // degrades to the child axis.
            if c == ':' && rest[idx + 1..].starts_with(':') {
                break;
            }
            end = idx + c.len_utf8();
        }
        if end == 0 {
            return None;
        }
        let name = rest[..end].to_owned();
        self.i += end;
        Some(name)
    }
}

pub(super) fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

pub(super) fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')
}
