// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Resource bounds for parsing untrusted input.
//!
//! Every limit here exists because some parser shipped without it and
//! was exploited. They are not tuning knobs; they are the difference
//! between a parse error and a denial of service.
//!
//! The defaults are chosen to accept every document a human would write
//! and reject the shapes that only appear in attacks. A caller
//! processing documents they produced themselves can raise them; a
//! caller reading from the network should generally lower them.
//!
//! # Why bounds, and not just "safe Rust"
//!
//! `#![forbid(unsafe_code)]` prevents memory-unsafety. It does not
//! prevent a parser from allocating gigabytes, recursing until the
//! stack is gone, or spending ten minutes in a quadratic loop. Two
//! HIGH-severity advisories against another Rust XML crate in 2026
//! (RUSTSEC-2026-0194 and -0195) were exactly this: an O(N²)
//! duplicate-attribute scan and an unbounded namespace allocation, both
//! in entirely safe code.

/// Which edition of XML 1.0 to apply.
///
/// The 5th edition replaced the enumerated character classes of
/// Appendix B with a small set of broad ranges. The two **disagree** —
/// they are not a superset relation — so a name legal under one may be
/// illegal under the other. Nearly all documents are unaffected; those
/// that are not are exactly what the W3C suite's 309 edition-specific
/// tests probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Edition {
    /// XML 1.0 4th edition and earlier: `Letter | '_' | ':'`, where
    /// `Letter` is Appendix B's `BaseChar | Ideographic`.
    Fourth,
    /// XML 1.0 5th edition, the current one, and XML 1.1.
    #[default]
    Fifth,
}

/// Resource bounds applied while parsing.
///
/// Construct with [`Limits::default`] and adjust, or start from
/// [`Limits::permissive`] / [`Limits::strict`].
///
/// This type is `#[non_exhaustive]`: new limits will be added as new
/// features land, and adding one must not be a breaking change.
/// Construct it from `default()` rather than with a struct literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Which edition's name rules to apply.
    ///
    /// Default: [`Edition::Fifth`].
    pub edition: Edition,

    /// Maximum element nesting depth.
    ///
    /// Parsing descends one stack frame per open element, so an
    /// unbounded document exhausts the stack — and a stack overflow
    /// aborts the process rather than unwinding, so no caller can
    /// catch it.
    ///
    /// Default: `256`. For comparison, Woodstox defaults to 1000 and the
    /// JDK's `maxElementDepth` to 100.
    ///
    /// # This limit is bounded by the stack, not by policy
    ///
    /// Parsing is recursive descent, so each open element costs a stack
    /// frame. Measured on this parser:
    ///
    /// | Build | Stack | Max depth | Bytes/frame |
    /// |---|---|---|---|
    /// | release | 2 MiB | 1,937 | ~1,086 |
    /// | release | 8 MiB | 7,720 | ~1,086 |
    /// | debug | 2 MiB | **280** | ~7,489 |
    /// | debug | 8 MiB | 1,121 | ~7,483 |
    ///
    /// A debug build on a 2 MiB thread — which is what `cargo test`
    /// gives you — survives only about 280 levels. That is why the
    /// default is 256 and why [`Limits::permissive`] does **not** raise
    /// it: exceeding the real ceiling aborts the process rather than
    /// returning an error, so a generous-looking value here would
    /// defeat the protection it appears to provide.
    ///
    /// To go deeper, run the parse on a thread with a larger stack and
    /// budget roughly 7.5 KiB per level for a debug build:
    ///
    /// ```no_run
    /// # use oxml::{Limits, parse_with};
    /// let mut limits = Limits::default();
    /// limits.max_depth = 2_000;
    /// std::thread::Builder::new()
    ///     .stack_size(32 * 1024 * 1024)
    ///     .spawn(move || parse_with("<a/>", limits))
    ///     .expect("spawn");
    /// ```
    pub max_depth: usize,

    /// Maximum number of attributes on a single element.
    ///
    /// Guards the duplicate-attribute check, which must compare each
    /// new name against those already seen.
    ///
    /// Default: 1000, matching Woodstox's
    /// `maxAttributesPerElement`.
    pub max_attributes_per_element: usize,

    /// Maximum length in bytes of a single attribute value.
    ///
    /// Default: `524_288`, matching Woodstox's `maxAttributeSize`.
    pub max_attribute_size: usize,

    /// Maximum length in bytes of an element, attribute, PI or
    /// DTD-declared name.
    ///
    /// Default: `8192`. The JDK's `maxXMLNameLimit` is 1000, which
    /// the W3C suite shows to be too low: `ibm85v01` is a valid
    /// document whose processing-instruction target enumerates every
    /// legal `BaseChar` in a single 4,250-byte name.
    pub max_name_length: usize,

    /// Maximum number of nodes in the resulting document, if any.
    ///
    /// The most direct bound on memory: the arena cannot exceed this
    /// many entries. `None` means unbounded.
    ///
    /// Default: `None`.
    pub max_nodes: Option<usize>,

    /// Maximum length in bytes of a single text or CDATA node, if any.
    ///
    /// Default: `None`.
    pub max_text_length: Option<usize>,

    /// Maximum nesting depth of an `XPath` expression.
    ///
    /// An expression is untrusted input in every front end of this
    /// crate: the CLI takes one from a shell, the MCP server from a
    /// model, the WASM bindings from JavaScript. Compilation is
    /// recursive descent, so `((((…))))` exhausts the stack.
    ///
    /// Counts nested parentheses, predicates and function arguments;
    /// an expression nested more levels than this is a compile error.
    /// Applied by `XPath::compile_with_limits`; `XPath::compile` uses
    /// the default.
    ///
    /// Default: `256`.
    pub max_xpath_depth: usize,

    /// Maximum nesting depth of general entity expansion.
    ///
    /// The billion-laughs bound. Ten nested entities each referring to
    /// the previous ten times is 10^10 characters from a few hundred
    /// bytes of input.
    ///
    /// Default: `10`, matching roxmltree and the JDK's posture.
    pub max_entity_depth: usize,

    /// Maximum total characters produced by expanding entities in one
    /// document.
    ///
    /// Depth alone does not bound the quadratic-blowup variant, where a
    /// single large entity is referenced many times at depth one.
    ///
    /// Default: `10_000_000`.
    pub max_entity_expansion: usize,

    /// Maximum number of operators and steps in one `XPath` expression.
    ///
    /// Bounds compilation work independently of nesting. The JDK
    /// enforces an analogous `xpathTotalOpLimit`; no Rust `XPath`
    /// implementation does.
    ///
    /// Counts every binary operator, unary minus, location step,
    /// predicate and function call; one more than this is a compile
    /// error. Applied by `XPath::compile_with_limits`;
    /// `XPath::compile` uses the default.
    ///
    /// Default: `10_000`.
    pub max_xpath_operators: usize,

    /// Maximum total bytes of document type declaration text the DTD
    /// parser will read, if any.
    ///
    /// The internal subset, an external subset the caller supplied,
    /// and every external parameter entity fetched through
    /// [`ExternalSource`] all count towards one budget. Declarations
    /// parse in linear time, so this is a ceiling on memory rather
    /// than on time: an external source can hand back a subset of any
    /// size, and nothing else bounds how much of it is held.
    ///
    /// `None` means unbounded. Default: `Some(16 MiB)`, which no DTD
    /// in the W3C conformance suite approaches. Woodstox and the JDK
    /// have no direct equivalent; the JDK's `totalEntitySizeLimit`
    /// (50,000,000) is the nearest.
    ///
    /// [`ExternalSource`]: crate::external::ExternalSource
    pub max_dtd_bytes: Option<usize>,

    /// Maximum total bytes of replacement text produced by expanding
    /// parameter entities in one document.
    ///
    /// The parameter-entity form of the billion-laughs bound, charged
    /// the way [`Limits::max_entity_expansion`] is. Depth alone does
    /// not catch the width variant: three entities of a hundred
    /// references each produce ten thousand copies of the innermost
    /// one from under a kilobyte of declarations, three levels deep.
    ///
    /// Default: `10_000_000`, the same as `max_entity_expansion`.
    pub max_pe_expansion: usize,

    /// Maximum bytes of a single construct the streaming reader will
    /// buffer while waiting for its end, if any.
    ///
    /// [`Reader::from_reader`] and `AsyncReader` keep only the
    /// construct being read: a start tag up to its `>`, a comment up
    /// to its `-->`, a run of character data up to the next `<`. The
    /// scanner is never handed a construct that is not whole, so
    /// neither [`Limits::max_text_length`] nor
    /// [`Limits::max_attribute_size`] can fire until the construct is
    /// complete -- and a source that never closes one would grow the
    /// buffer without bound. This is the cap on that buffer; a
    /// construct that exceeds it is refused with
    /// [`ErrorKind::ConstructTooLarge`].
    ///
    /// Only the incremental readers apply it. [`Reader::new`] and
    /// [`crate::parse`] are handed the whole document, so there is
    /// nothing left to bound.
    ///
    /// `None` means unbounded. Default: `Some(64 MiB)`. Woodstox has
    /// no equivalent because it reports a text node in pieces; this
    /// reader reports a run whole so the events match the tree.
    ///
    /// [`Reader::from_reader`]: crate::stream::Reader::from_reader
    /// [`Reader::new`]: crate::stream::Reader::new
    /// [`ErrorKind::ConstructTooLarge`]: crate::ErrorKind::ConstructTooLarge
    pub max_construct_bytes: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            edition: Edition::Fifth,
            max_depth: 256,
            max_attributes_per_element: 1_000,
            max_attribute_size: 524_288,
            max_name_length: 8_192,
            max_nodes: None,
            max_text_length: None,
            max_entity_depth: 10,
            max_entity_expansion: 10_000_000,
            max_xpath_depth: 256,
            max_xpath_operators: 10_000,
            max_dtd_bytes: Some(16 * 1024 * 1024),
            max_pe_expansion: 10_000_000,
            max_construct_bytes: Some(64 * 1024 * 1024),
        }
    }
}

impl Limits {
    /// Bounds suitable for input you produced yourself.
    ///
    /// Raises every cap substantially and removes the optional ones.
    /// Do not use this on input arriving from a network.
    ///
    /// **`max_depth` is deliberately left at the default.** It is
    /// bounded by the thread's stack rather than by policy, and
    /// exceeding the real ceiling aborts the process instead of
    /// returning an error — so raising it here would hand out a
    /// crash, not permissiveness. See [`Limits::max_depth`] for the
    /// measured ceilings and how to raise it safely.
    #[must_use]
    pub fn permissive() -> Self {
        Self {
            edition: Edition::Fifth,
            max_depth: Self::default().max_depth,
            max_attributes_per_element: 100_000,
            max_attribute_size: 64 * 1024 * 1024,
            max_name_length: 1024 * 1024,
            max_nodes: None,
            max_text_length: None,
            max_entity_depth: 40,
            max_entity_expansion: 1_000_000_000,
            max_xpath_depth: 1_000,
            max_xpath_operators: 1_000_000,
            max_dtd_bytes: None,
            max_pe_expansion: 1_000_000_000,
            max_construct_bytes: None,
        }
    }

    /// Bounds suitable for small documents from an untrusted source.
    ///
    /// Tight enough that a single document cannot consume meaningful
    /// memory or time. Will reject some legitimate large documents —
    /// that is the intent.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            edition: Edition::Fifth,
            max_depth: 64,
            max_attributes_per_element: 64,
            max_attribute_size: 8 * 1024,
            max_name_length: 256,
            max_nodes: Some(100_000),
            max_text_length: Some(1024 * 1024),
            max_entity_depth: 4,
            max_entity_expansion: 100_000,
            max_xpath_depth: 32,
            max_xpath_operators: 1_000,
            max_dtd_bytes: Some(64 * 1024),
            max_pe_expansion: 100_000,
            max_construct_bytes: Some(1024 * 1024),
        }
    }
}
