// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `XPath` 1.0.
//!
//! The only `XPath` implementation on crates.io, `sxd-xpath`, has not
//! been released since 2018. This module exists to close that gap:
//! compile an expression once with [`XPath::compile`], then evaluate
//! it against as many documents as you like.

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
    /// Returns [`XPathError`] if the expression is malformed.
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
