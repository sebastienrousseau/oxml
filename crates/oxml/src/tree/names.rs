// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Names and text: what the mutation API accepts, and how it is
//! stored.
//!
//! A name must be an `NCName`. Character data must be something
//! some version of XML can write; a comment or processing
//! instruction is written verbatim and is held to what may appear
//! literally. Text the input does not contain goes in the
//! document's side table, overwritten in place on replacement and
//! released for reuse on removal.

use alloc::string::String;

use super::{Chars, Document, ExpandedName, NameId, NodeData, NodeError};
use crate::parser::{is_name_char, is_name_start, is_xml_char};

/// Whether `s` is an `NCName`: a `Name` with no colon.
pub(super) fn is_ncname(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c != ':' && is_name_start(c))
        && chars.all(|c| c != ':' && is_name_char(c))
}

/// A prefix that may be declared: empty (the default namespace), or
/// an `NCName` that is not reserved.
pub(super) fn check_prefix(prefix: &str) -> Result<(), NodeError> {
    if prefix.is_empty()
        || (is_ncname(prefix) && prefix != "xml" && prefix != "xmlns")
    {
        Ok(())
    } else {
        Err(NodeError::InvalidName)
    }
}

/// Whether some version of XML can write `c`, literally or as a
/// reference.
///
/// XML 1.1's `Char` production admits everything but `NUL`, the two
/// non-characters, and surrogates -- which a `char` cannot hold.
pub(super) const fn is_expressible(c: char) -> bool {
    !matches!(c, '\0' | '\u{fffe}' | '\u{ffff}')
}

/// Character data or an attribute value: escaped on the way out, so
/// anything expressible is accepted.
pub(super) fn check_text(text: &str) -> Result<(), NodeError> {
    if text.chars().all(is_expressible) {
        Ok(())
    } else {
        Err(NodeError::InvalidText)
    }
}

/// A comment or processing-instruction body: written verbatim, so
/// held to what may appear literally in XML 1.0.
pub(super) fn check_literal(text: &str) -> Result<(), NodeError> {
    if text.chars().all(is_xml_char) {
        Ok(())
    } else {
        Err(NodeError::InvalidText)
    }
}

pub(super) fn check_comment(text: &str) -> Result<(), NodeError> {
    check_literal(text)?;
    if text.contains("--") || text.ends_with('-') {
        return Err(NodeError::InvalidText);
    }
    Ok(())
}

pub(super) fn check_pi(target: &str, data: &str) -> Result<(), NodeError> {
    if !is_ncname(target) || target.eq_ignore_ascii_case("xml") {
        return Err(NodeError::InvalidName);
    }
    check_literal(data)?;
    if data.contains("?>") {
        return Err(NodeError::InvalidText);
    }
    Ok(())
}

/// The empty namespace name is no namespace.
pub(super) fn namespace_of(namespace: Option<&str>) -> Option<&str> {
    namespace.filter(|u| !u.is_empty())
}

impl Document {
    /// Intern a name, reusing the entry if the document already has it.
    ///
    /// Names are interned because a document repeats a handful of them
    /// thousands of times. A linear scan is right here: mutation
    /// touches few names, and a map would cost every parsed document
    /// memory to speed up a case most documents never reach.
    ///
    /// Matched by expanded name, so a name the parser interned with a
    /// prefix is reused and keeps that prefix for the serialiser.
    pub(super) fn intern(
        &mut self,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<NameId, NodeError> {
        if !is_ncname(local) {
            return Err(NodeError::InvalidName);
        }
        let namespace = namespace_of(namespace);
        let found = self.names.iter().position(|n| {
            n.namespace.as_deref() == namespace && n.local == local
        });
        if let Some(i) = found {
            return Ok(NameId(u32::try_from(i).unwrap_or(u32::MAX)));
        }
        let id = u32::try_from(self.names.len())
            .expect("document exceeds u32::MAX names");
        // `String::from` rather than `.to_owned()`: the `ToOwned`
        // trait is not in the prelude without `std`, and this crate
        // builds for three bare-metal targets.
        self.names.push(ExpandedName {
            namespace: namespace.map(String::from),
            local: String::from(local),
        });
        self.name_prefixes.push(None);
        Ok(NameId(id))
    }

    /// Store text the input does not contain, returning a handle.
    ///
    /// Reuses an entry released by a removal before growing the table.
    pub(super) fn store_text(&mut self, text: &str) -> Chars {
        if text.is_empty() {
            return Chars::EMPTY;
        }
        if let Some(i) = self.free_expanded.pop() {
            if let Some(slot) = self.expanded.get_mut(i as usize) {
                slot.push_str(text);
                return Chars::Expanded(i);
            }
        }
        let id = u32::try_from(self.expanded.len())
            .expect("document exceeds u32::MAX texts");
        self.expanded.push(String::from(text));
        Chars::Expanded(id)
    }

    /// Replace stored text, overwriting its entry in place when it has
    /// one.
    ///
    /// Pushing a new entry per replacement retained every old value:
    /// a thousand replacements of a 10 KB value held 10 MB.
    pub(super) fn replace_text(&mut self, old: Chars, text: &str) -> Chars {
        if let Chars::Expanded(i) = old {
            if let Some(slot) = self.expanded.get_mut(i as usize) {
                slot.clear();
                slot.push_str(text);
                return old;
            }
        }
        self.store_text(text)
    }

    /// Give an expanded-text entry back for reuse.
    pub(super) fn release_chars(&mut self, chars: Chars) {
        if let Chars::Expanded(i) = chars {
            if let Some(slot) = self.expanded.get_mut(i as usize) {
                // A fresh `String` rather than `clear`, which keeps the
                // capacity: the point is to stop holding the bytes.
                *slot = String::new();
                self.free_expanded.push(i);
            }
        }
    }

    /// Release whatever expanded text the node in `index` holds.
    pub(super) fn release_node_text(&mut self, index: usize) {
        let held = match self.nodes.get(index).map(|n| &n.data) {
            Some(NodeData::Attr { value, .. }) => [Some(*value), None],
            Some(NodeData::Text(c) | NodeData::Comment(c)) => [Some(*c), None],
            Some(NodeData::Namespace { prefix, uri }) => {
                [Some(*prefix), Some(*uri)]
            }
            Some(NodeData::ProcessingInstruction { target, data }) => {
                [Some(*target), Some(*data)]
            }
            _ => [None, None],
        };
        for chars in held.into_iter().flatten() {
            self.release_chars(chars);
        }
    }
}
