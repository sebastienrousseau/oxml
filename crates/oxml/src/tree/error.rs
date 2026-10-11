// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Why a mutation was refused.

/// Why a mutation could not be applied.
///
/// Every variant names a condition the caller can check for, rather
/// than a generic failure: a mutation API that returns one opaque
/// error teaches callers to `unwrap` it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeError {
    /// The node behind this identifier has been removed.
    ///
    /// Carries nothing that would let a caller retry with the same
    /// identifier, because retrying is never the right response.
    Stale,
    /// The operation needs an element and was given something else --
    /// a text node, a comment, or the document root.
    NotAnElement,
    /// The operation needs somewhere to put children, and only the
    /// document root and elements have children.
    ///
    /// A child appended under a text node was once stored and then
    /// silently dropped by the serialiser, which is the worst of both:
    /// the call succeeded and the document lost the node.
    NotAContainer,
    /// The node is an attribute or namespace node.
    ///
    /// Those belong to their element rather than to a child list, so
    /// they cannot be moved, inserted or detached. Remove one with
    /// [`Document::remove`](super::Document::remove) or [`Document::remove_attribute`](super::Document::remove_attribute).
    NotAChild,
    /// The operation needs a text node.
    NotText,
    /// The node is the document root, which cannot be removed, moved
    /// or detached: without it there is no document.
    NotRemovable,
    /// The document already has a root element.
    ///
    /// XML permits exactly one. A second would serialise to something
    /// this crate's own parser rejects as trailing content, which the
    /// caller would not discover until the round trip.
    RootElementExists,
    /// The move would place a node inside its own subtree.
    ///
    /// The result would be a tree containing a loop, and every walker
    /// here follows children until they run out. None of them would.
    WouldCycle,
    /// The name is not an `NCName`: empty, starting with a digit,
    /// containing a space or a colon, or a reserved prefix.
    ///
    /// Namespaced names are given as a namespace URI and a local
    /// part; a prefix is chosen when the document is written.
    InvalidName,
    /// The text cannot be written in any version of XML.
    ///
    /// For character data and attribute values that is `NUL`, `U+FFFE`
    /// and `U+FFFF`; a control character is fine and goes out as a
    /// reference. A comment or processing instruction is written
    /// verbatim, so it is also held to what may appear literally:
    /// no control characters, no `--` in a comment, no `?>` in a
    /// processing instruction.
    InvalidText,
}

impl NodeError {
    /// Every variant, in declaration order.
    #[cfg(test)]
    const ALL: [Self; 10] = [
        Self::Stale,
        Self::NotAnElement,
        Self::NotAContainer,
        Self::NotAChild,
        Self::NotText,
        Self::NotRemovable,
        Self::RootElementExists,
        Self::WouldCycle,
        Self::InvalidName,
        Self::InvalidText,
    ];

    /// What went wrong, in a sentence.
    ///
    /// Indexed by discriminant rather than matched: the table reads
    /// in the same order as the declaration above, and a test checks
    /// every variant has a message of its own.
    const fn message(self) -> &'static str {
        const MESSAGES: [&str; 10] = [
            "the node has been removed",
            "the node is not an element",
            "the node cannot have children",
            "an attribute or namespace node cannot be moved or detached",
            "the node is not a text node",
            "the document root cannot be removed, moved or detached",
            "the document already has a root element",
            "the move would place a node inside its own subtree",
            "the name is not an NCName",
            "the text cannot be written as XML",
        ];
        MESSAGES[self as usize]
    }
}

impl core::fmt::Display for NodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for NodeError {}

#[cfg(test)]
mod error_tests {
    use super::NodeError;

    #[test]
    fn every_error_has_its_own_message() {
        let messages: alloc::vec::Vec<&str> =
            NodeError::ALL.iter().map(|e| e.message()).collect();
        for (i, m) in messages.iter().enumerate() {
            assert!(!m.is_empty());
            assert!(
                !messages[..i].contains(m),
                "{:?} shares a message with an earlier variant",
                NodeError::ALL[i]
            );
        }
        assert_eq!(NodeError::NotText.message(), "the node is not a text node");
        assert_eq!(
            NodeError::InvalidText.message(),
            "the text cannot be written as XML"
        );
    }
}
