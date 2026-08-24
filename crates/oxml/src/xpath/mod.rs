// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `XPath` 1.0.
//!
//! The only other pure-Rust `XPath` implementation on crates.io,
//! `sxd-xpath`, has not been released since 2018; the `libxml`
//! bindings offer one, at the cost of linking C. This module exists to
//! close that gap: compile an expression once with
//! [`XPath::compile`], then evaluate it against as many documents as
//! you like.

mod ast;
mod eval;
mod float;
mod parser;

pub use ast::{Axis, BinaryOp, Expr, NodeTest, Step};
pub use eval::Value;
pub use parser::XPathError;

use crate::tree::{Document, NodeId};

/// A compiled `XPath` expression.
///
/// Compiling is separated from evaluation because the compiled form is
/// document-independent and reusable — evaluating the same expression
/// across a thousand documents should parse it once, not a thousand
/// times.
#[derive(Debug, Clone, PartialEq)]
pub struct XPath {
    expr: Expr,
}

impl XPath {
    /// Compile an expression.
    ///
    /// # Errors
    ///
    /// Returns [`XPathError`] if the expression is malformed, names a
    /// function outside `XPath` 1.0's library, or passes a function
    /// the wrong number of arguments. The last two are compile errors
    /// rather than empty results on purpose: an unknown name that
    /// evaluated to nothing was indistinguishable from a document with
    /// no match.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxml::XPath;
    /// let xp = XPath::compile("//book[@lang='en']/title")?;
    /// # Ok::<(), oxml::XPathError>(())
    /// ```
    pub fn compile(expr: &str) -> Result<Self, XPathError> {
        Ok(Self {
            expr: parser::compile(expr)?,
        })
    }

    /// Compile with namespace prefixes bound to URIs.
    ///
    /// `XPath` 1.0 resolves a prefix in an expression against the
    /// *expression's* context rather than the document's declarations.
    /// The same prefix can mean different things in the two, and
    /// resolving against the document would make an expression's
    /// meaning depend on which document it ran against.
    ///
    /// A prefix the expression uses and this list does not bind is a
    /// compile error. `xml` is bound by specification and need not be
    /// given.
    ///
    /// # Errors
    ///
    /// Returns [`XPathError`] as [`XPath::compile`] does, and
    /// additionally if the expression uses a prefix these bindings do
    /// not cover.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxml::{XPath, parse};
    ///
    /// let doc = parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a><a>no</a></r>"#)?;
    /// let xp = XPath::compile_with_namespaces("//m:a", &[("m", "urn:u")])?;
    /// assert_eq!(xp.evaluate(&doc).to_str(&doc), "yes");
    ///
    /// // The prefix in the expression is independent of the one in the
    /// // document: only the URI has to match.
    /// let xp = XPath::compile_with_namespaces("//q:a", &[("q", "urn:u")])?;
    /// assert_eq!(xp.evaluate(&doc).to_str(&doc), "yes");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn compile_with_namespaces(
        expr: &str,
        namespaces: &[(&str, &str)],
    ) -> Result<Self, XPathError> {
        Ok(Self {
            expr: parser::compile_with(expr, namespaces)?,
        })
    }

    /// Evaluate against a document, starting at its root.
    #[must_use]
    pub fn evaluate(&self, doc: &Document) -> Value {
        eval::evaluate(doc, &self.expr, doc.root())
    }

    /// Evaluate with an explicit context node.
    #[must_use]
    pub fn evaluate_from(&self, doc: &Document, context: NodeId) -> Value {
        eval::evaluate(doc, &self.expr, context)
    }

    /// The compiled expression tree.
    ///
    /// Exposed for tooling that wants to inspect or rewrite queries
    /// rather than only run them.
    #[must_use]
    pub const fn expr(&self) -> &Expr {
        &self.expr
    }
}
