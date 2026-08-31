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
mod eval;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
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

/// A type an [`XPath`] result can be extracted into.
///
/// Implemented for `String`, `f64`, `i64`, `bool` and
/// [`NodeId`]. The conversions follow `XPath` 1.0's own
/// rules with one deliberate exception, documented on the `f64`
/// implementation: a value that is not a number is an error rather
/// than `NaN`, because a caller who names a numeric type wants a
/// number, not a value that poisons every comparison downstream.
pub trait FromXPath: Sized {
    /// Extract `Self` from an evaluated value.
    ///
    /// # Errors
    ///
    /// [`TypeError`] when the value cannot represent `Self` -- what
    /// that means is documented per implementation.
    fn from_value(value: &Value, doc: &Document) -> Result<Self, TypeError>;
}

/// Why a value could not be extracted as the requested type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    /// What went wrong, in a form a person can act on.
    pub message: String,
}

impl core::fmt::Display for TypeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TypeError {}

impl FromXPath for String {
    /// `XPath`'s `string()` conversion, which cannot fail.
    fn from_value(value: &Value, doc: &Document) -> Result<Self, TypeError> {
        Ok(value.to_str(doc))
    }
}

impl FromXPath for f64 {
    /// `XPath`'s `number()` conversion, except that `NaN` is an error.
    ///
    /// The specification says a non-numeric string converts to `NaN`.
    /// That is the right behaviour *inside* an expression, where
    /// `NaN`'s comparison rules are part of the language, and this
    /// crate's `Value::to_number` implements it faithfully. At the
    /// boundary into Rust it is the wrong default: a caller writing
    /// `xpath_one::<f64>("//price")` wants the price, and handing back
    /// `NaN` defers the failure to whatever arithmetic touches it
    /// next, stripped of any hint of where it came from.
    fn from_value(value: &Value, doc: &Document) -> Result<Self, TypeError> {
        let n = value.to_number(doc);
        if n.is_nan() {
            return Err(TypeError {
                message: format!("`{}` is not a number", value.to_str(doc)),
            });
        }
        Ok(n)
    }
}

impl FromXPath for i64 {
    /// An `f64` extraction that must also be integral and in range.
    ///
    /// `XPath` 1.0 has no integer type -- every number is an `f64` --
    /// so this is a convenience for the common case of counts and
    /// years. `1.5` is an error, not a truncation: a caller asking for
    /// an integer should not silently receive the floor of something
    /// that was not one.
    fn from_value(value: &Value, doc: &Document) -> Result<Self, TypeError> {
        let n = f64::from_value(value, doc)?;
        // `fract` lives in std, not core; `float::trunc` is the shim
        // this module routes every such call through, so the no_std
        // build breaks in one file rather than a dozen.
        //
        // `n != trunc(n)` is an exact integrality test -- the one case
        // where comparing floats for equality is right rather than
        // sloppy. An epsilon would misclassify values near an integer.
        #[allow(clippy::float_cmp)]
        let fractional = n != float::trunc(n);
        if fractional {
            return Err(TypeError {
                message: format!("`{n}` is not an integer"),
            });
        }
        // Beyond 2^53 an f64 no longer represents every integer, so a
        // value out there may not be the integer it appears to be.
        if n.abs() > 9_007_199_254_740_992.0 {
            return Err(TypeError {
                message: format!(
                    "`{n}` is outside the exactly-representable range"
                ),
            });
        }
        #[allow(clippy::cast_possible_truncation)]
        Ok(n as Self)
    }
}

impl FromXPath for bool {
    /// `XPath`'s `boolean()` conversion, which cannot fail.
    fn from_value(value: &Value, doc: &Document) -> Result<Self, TypeError> {
        let _ = doc;
        Ok(value.to_boolean())
    }
}

impl FromXPath for crate::NodeId {
    /// The first node of a node-set, in document order.
    ///
    /// An empty node-set is an error, and so is a value that is not a
    /// node-set at all -- `string(//a)` produces a string, and there
    /// is no node to hand back.
    fn from_value(value: &Value, doc: &Document) -> Result<Self, TypeError> {
        let _ = doc;
        match value.nodes() {
            Some([first, ..]) => Ok(*first),
            Some([]) => Err(TypeError {
                message: String::from("the expression matched no nodes"),
            }),
            None => Err(TypeError {
                message: String::from(
                    "the expression does not produce a node-set",
                ),
            }),
        }
    }
}

/// Why a typed query failed: either the expression, or the type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// The expression did not compile.
    Compile(XPathError),
    /// The expression evaluated, but its value cannot represent the
    /// requested type.
    Type(TypeError),
}

impl core::fmt::Display for QueryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Compile(e) => write!(f, "{e}"),
            Self::Type(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for QueryError {}

impl Document {
    /// Evaluate an expression and extract one typed value.
    ///
    /// ```
    /// # #[cfg(feature = "xpath")] {
    /// let doc = oxml::parse("<order><price>9.99</price><qty>3</qty></order>").unwrap();
    /// let price: f64 = doc.xpath_one("number(//price)").unwrap();
    /// let qty: i64 = doc.xpath_one("number(//qty)").unwrap();
    /// let has_discount: bool = doc.xpath_one("count(//discount) > 0").unwrap();
    /// assert_eq!(price, 9.99);
    /// assert_eq!(qty, 3);
    /// assert!(!has_discount);
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`QueryError::Compile`] if the expression is invalid,
    /// [`QueryError::Type`] if its value cannot represent `T` -- for
    /// `f64` that includes `NaN`, deliberately; see
    /// [`FromXPath`].
    pub fn xpath_one<T: FromXPath>(&self, expr: &str) -> Result<T, QueryError> {
        let compiled = XPath::compile(expr).map_err(QueryError::Compile)?;
        let value = compiled.evaluate(self);
        T::from_value(&value, self).map_err(QueryError::Type)
    }

    /// Evaluate an expression and extract every matched node as `T`.
    ///
    /// The expression must produce a node-set; each node's
    /// string-value is then converted independently, so one
    /// unconvertible node fails the whole call rather than being
    /// silently skipped.
    ///
    /// ```
    /// # #[cfg(feature = "xpath")] {
    /// let doc = oxml::parse(
    ///     "<cart><item price='9.99'/><item price='7.50'/></cart>",
    /// ).unwrap();
    /// let prices: Vec<f64> = doc.xpath_all("//item/@price").unwrap();
    /// assert_eq!(prices, [9.99, 7.5]);
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`QueryError::Compile`] if the expression is invalid,
    /// [`QueryError::Type`] if it does not produce a node-set or any
    /// node's value cannot represent `T`.
    pub fn xpath_all<T: FromXPath>(
        &self,
        expr: &str,
    ) -> Result<Vec<T>, QueryError> {
        let compiled = XPath::compile(expr).map_err(QueryError::Compile)?;
        let value = compiled.evaluate(self);
        let Some(nodes) = value.nodes() else {
            return Err(QueryError::Type(TypeError {
                message: String::from(
                    "the expression does not produce a node-set",
                ),
            }));
        };
        nodes
            .iter()
            .map(|id| {
                let single = Value::NodeSet(alloc::vec![*id]);
                T::from_value(&single, self).map_err(QueryError::Type)
            })
            .collect()
    }
}
