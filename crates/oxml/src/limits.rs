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

    /// Maximum length in bytes of an element, attribute or PI name.
    ///
    /// Default: 1000, matching the JDK's `maxXMLNameLimit`.
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
    /// Default: `256`.
    pub max_xpath_depth: usize,

    /// Maximum number of operators and steps in one `XPath` expression.
    ///
    /// Bounds compilation work independently of nesting. The JDK
    /// enforces an analogous `xpathTotalOpLimit`; no Rust `XPath`
    /// implementation does.
    ///
    /// Default: `10_000`.
    pub max_xpath_operators: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 256,
            max_attributes_per_element: 1_000,
            max_attribute_size: 524_288,
            max_name_length: 1_000,
            max_nodes: None,
            max_text_length: None,
            max_xpath_depth: 256,
            max_xpath_operators: 10_000,
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
            max_depth: Self::default().max_depth,
            max_attributes_per_element: 100_000,
            max_attribute_size: 64 * 1024 * 1024,
            max_name_length: 1024 * 1024,
            max_nodes: None,
            max_text_length: None,
            max_xpath_depth: 1_000,
            max_xpath_operators: 1_000_000,
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
            max_depth: 64,
            max_attributes_per_element: 64,
            max_attribute_size: 8 * 1024,
            max_name_length: 256,
            max_nodes: Some(100_000),
            max_text_length: Some(1024 * 1024),
            max_xpath_depth: 32,
            max_xpath_operators: 1_000,
        }
    }
}
