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
//!
//! One extension beyond `XPath` 1.0, documented as such: the `*:local`
//! name test (2.0's production) matches a local name in whatever
//! namespace, so a structural question does not require binding a
//! prefix for every namespace in the document. Everything else is
//! strictly 1.0, and an unprefixed name test still matches nodes in
//! *no* namespace only.

mod ast;
mod axes;
mod budget;
mod compare;
mod context;
mod display;
mod eval;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
mod float;
mod functions;
mod names;
mod parser;
mod typed;
mod value;

pub use ast::{Axis, BinaryOp, Expr, Filter, Func, NodeTest, Step};
pub use context::{EvalContext, EvalError, EvalLimits};
pub use parser::XPathError;
pub use typed::{FromXPath, QueryError, TypeError};
pub use value::Value;

use crate::Limits;
use crate::tree::{Document, ExpandedName, NodeId};

/// A compiled `XPath` expression.
///
/// Compiling is separated from evaluation because the compiled form is
/// document-independent and reusable — evaluating the same expression
/// across a thousand documents should parse it once, not a thousand
/// times.
///
/// `Display` renders the expression back as canonical unabbreviated
/// `XPath` 1.0 -- `//a[1]` as
/// `/descendant-or-self::node()/child::a[1]` -- using the namespace
/// bindings it was compiled with, so that compiling the rendering
/// under the same bindings gives back an equal `XPath`.
#[derive(Debug, Clone, PartialEq)]
pub struct XPath {
    expr: Expr,
    /// The prefix bindings the expression was compiled under, kept so
    /// that `Display` can write a namespaced name with its prefix.
    namespaces: Vec<(String, String)>,
    /// Every distinct variable the expression references.
    variables: Vec<ExpandedName>,
}

impl core::fmt::Display for XPath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        display::Render {
            namespaces: &self.namespaces,
        }
        .expr(&self.expr, f)
    }
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
        Self::compile_with_limits(expr, &Limits::default())
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
    /// let doc = parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a><a>no</a></r>"#).unwrap();
    /// let xp = XPath::compile_with_namespaces("//m:a", &[("m", "urn:u")]).unwrap();
    /// assert_eq!(xp.evaluate(&doc).to_str(&doc), "yes");
    ///
    /// // The prefix in the expression is independent of the one in the
    /// // document: only the URI has to match.
    /// let xp = XPath::compile_with_namespaces("//q:a", &[("q", "urn:u")]).unwrap();
    /// assert_eq!(xp.evaluate(&doc).to_str(&doc), "yes");
    /// ```
    pub fn compile_with_namespaces(
        expr: &str,
        namespaces: &[(&str, &str)],
    ) -> Result<Self, XPathError> {
        Self::compile_with_namespaces_and_limits(
            expr,
            namespaces,
            &Limits::default(),
        )
    }

    /// Compile under explicit [`Limits`].
    ///
    /// An expression is untrusted input in every front end of this
    /// crate, and two of its shapes cost more than their length
    /// suggests: nesting (`((((...))))`, `not(not(...))`, `a[b[c[...]]]`)
    /// drives recursion, and a long flat run of operators
    /// (`1+1+1+...`) drives work. [`Limits::max_xpath_depth`] bounds
    /// the first and [`Limits::max_xpath_operators`] the second,
    /// counting every binary operator, unary minus, location step,
    /// predicate and function call. The other fields of `Limits`
    /// concern documents and are not consulted here.
    ///
    /// [`XPath::compile`] is this with [`Limits::default`].
    ///
    /// # Errors
    ///
    /// Returns [`XPathError`] as [`XPath::compile`] does, and
    /// additionally when the expression exceeds either bound; the
    /// message says which, and the offset is where the bound was
    /// crossed.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxml::{Limits, XPath};
    ///
    /// let strict = Limits::strict();
    /// assert!(XPath::compile_with_limits("//book[@lang='en']", &strict).is_ok());
    ///
    /// let deep = format!("{}1{}", "(".repeat(40), ")".repeat(40));
    /// let e = XPath::compile_with_limits(&deep, &strict).unwrap_err();
    /// assert!(e.message.contains("deep"));
    /// ```
    pub fn compile_with_limits(
        expr: &str,
        limits: &Limits,
    ) -> Result<Self, XPathError> {
        Self::compile_with_namespaces_and_limits(expr, &[], limits)
    }

    /// Compile with namespace prefixes bound to URIs, under explicit
    /// [`Limits`].
    ///
    /// The bindings work as in [`XPath::compile_with_namespaces`] and
    /// the limits as in [`XPath::compile_with_limits`]; this is the
    /// one constructor the other three delegate to.
    ///
    /// # Errors
    ///
    /// Returns [`XPathError`] for a malformed expression, an unbound
    /// prefix, or an expression over either limit.
    pub fn compile_with_namespaces_and_limits(
        expr: &str,
        namespaces: &[(&str, &str)],
        limits: &Limits,
    ) -> Result<Self, XPathError> {
        let compiled = parser::compile(expr, namespaces, limits)?;
        Ok(Self {
            expr: compiled.expr,
            namespaces: namespaces
                .iter()
                .map(|(p, u)| (p.to_string(), u.to_string()))
                .collect(),
            variables: compiled.variables,
        })
    }

    /// Evaluate against a document, starting at its root.
    ///
    /// This cannot fail, as `XPath` 1.0 has no exceptions: a function
    /// given the wrong type answers its documented silent value
    /// (`count('x')` is `0`), and an expression that references
    /// variables evaluates to an empty node-set, since nothing binds
    /// them here. [`XPath::evaluate_with`] reports both as errors.
    ///
    /// A document mutated since its order cache was last refreshed
    /// (see [`Document::is_order_dirty`]) costs one extra pre-order
    /// walk per evaluation, so that node-sets still come back in
    /// document order; [`Document::document_order`] refreshes the
    /// cache once for a batch of mutations.
    #[must_use]
    pub fn evaluate(&self, doc: &Document) -> Value {
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("oxml.xpath.evaluate").entered();

        self.lenient(doc, doc.root())
    }

    /// Evaluate with an explicit context node.
    ///
    /// Infallible under the same rules as [`XPath::evaluate`]; a
    /// context node that no longer belongs to the document selects
    /// nothing.
    #[must_use]
    pub fn evaluate_from(&self, doc: &Document, context: NodeId) -> Value {
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("oxml.xpath.evaluate_from").entered();

        self.lenient(doc, context)
    }

    /// The infallible evaluation both entry points share.
    fn lenient(&self, doc: &Document, context: NodeId) -> Value {
        if !self.variables.is_empty() {
            return Value::NodeSet(Vec::new());
        }
        // Outside strict mode the evaluator reports nothing: every
        // type error has a silent value, and the only variables it
        // could fail to find were refused above.
        eval::evaluate(doc, &self.expr, context, &EvalContext::default(), false)
            .unwrap_or_else(|_| Value::NodeSet(Vec::new()))
    }

    /// Evaluate against a document with variables bound, reporting
    /// what [`XPath::evaluate`] answers silently.
    ///
    /// # Errors
    ///
    /// [`EvalError::UnboundVariable`] if the expression references a
    /// variable `ctx` does not bind; [`EvalError::TypeError`] if a
    /// function or predicate that requires a node-set is given
    /// another type.
    pub fn evaluate_with(
        &self,
        doc: &Document,
        ctx: &EvalContext,
    ) -> Result<Value, EvalError> {
        self.evaluate_from_with(doc, doc.root(), ctx)
    }

    /// [`XPath::evaluate_with`] from an explicit context node.
    ///
    /// # Errors
    ///
    /// As [`XPath::evaluate_with`], and [`EvalError::Stale`] if
    /// `context` does not belong to `doc`.
    pub fn evaluate_from_with(
        &self,
        doc: &Document,
        context: NodeId,
        ctx: &EvalContext,
    ) -> Result<Value, EvalError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("oxml.xpath.evaluate_with").entered();

        if doc.kind(context).is_none() {
            return Err(EvalError::Stale);
        }
        eval::evaluate(doc, &self.expr, context, ctx, true)
    }

    /// Every distinct variable the expression references, in order of
    /// first reference. Empty for an expression that
    /// [`XPath::evaluate`] can answer in full.
    #[must_use]
    pub fn variables(&self) -> &[ExpandedName] {
        &self.variables
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
