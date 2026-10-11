// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Compiling `XPath` text into an [`Expr`].
//!
//! A recursive-descent parser following the `XPath` 1.0 grammar's
//! precedence ladder: `or` binds loosest, then `and`, equality,
//! relational, additive, multiplicative, unary, union, and finally
//! paths. The six binary levels are one precedence-climbing function
//! rather than one function each, so that a nested sub-expression
//! costs five stack frames instead of ten: an expression is untrusted
//! input, and [`Limits::max_xpath_depth`] levels of nesting have to
//! fit on a small thread's stack before the limit refuses the next.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

mod binary;
mod lex;
mod node_test;

use binary::chain;

use super::ast::{Axis, BinaryOp, Expr, Filter, Func, NodeTest, Step};
use crate::Limits;
use crate::tree::ExpandedName;

/// Why an `XPath` expression could not be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XPathError {
    /// A human-readable description.
    pub message: String,
    /// Byte offset into the expression.
    pub offset: usize,
}

impl core::fmt::Display for XPathError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "at {}: {}", self.offset, self.message)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for XPathError {}

struct P<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    /// How many nested sub-expressions are open. Parsing is recursive
    /// descent, so `((((...))))` would otherwise exhaust the stack —
    /// and an expression is untrusted input in every front end of this
    /// crate. Bounded by `max_depth`, from
    /// [`Limits::max_xpath_depth`].
    depth: usize,
    /// The nesting depth at which the expression is refused.
    max_depth: usize,
    /// How many operators, steps, predicates and calls have been
    /// parsed so far. Nesting alone does not bound work: `1+1+1+...`
    /// is flat, and both compiling and evaluating it cost in
    /// proportion to its length.
    ops: usize,
    /// The operator count at which the expression is refused, from
    /// [`Limits::max_xpath_operators`].
    max_ops: usize,
    /// Prefix-to-URI bindings for name tests.
    ///
    /// `XPath` 1.0 resolves a prefix against the *expression's* context,
    /// not the document's declarations -- the same prefix can mean
    /// different things in the two, and resolving against the document
    /// would make an expression's meaning depend on which document it
    /// ran against.
    namespaces: &'a [(&'a str, &'a str)],
    /// Every distinct variable the expression references, in order
    /// of first reference.
    variables: Vec<ExpandedName>,
}

/// A compiled expression and what it needs from its evaluation
/// context.
pub struct Compiled {
    /// The expression tree.
    pub expr: Expr,
    /// Every distinct variable the expression references.
    pub variables: Vec<ExpandedName>,
}

/// Compile an `XPath` 1.0 expression with namespace prefixes bound
/// to URIs, under `limits`.
///
/// Only [`Limits::max_xpath_depth`] and
/// [`Limits::max_xpath_operators`] apply to an expression; the other
/// fields bound documents.
///
/// # Errors
///
/// Returns [`XPathError`] if the expression is malformed, uses a
/// prefix that `namespaces` does not bind, or exceeds either limit.
pub fn compile(
    expr: &str,
    namespaces: &[(&str, &str)],
    limits: &Limits,
) -> Result<Compiled, XPathError> {
    let mut p = P {
        s: expr,
        b: expr.as_bytes(),
        i: 0,
        depth: 0,
        max_depth: limits.max_xpath_depth,
        ops: 0,
        max_ops: limits.max_xpath_operators,
        namespaces,
        variables: Vec::new(),
    };
    let e = p.parse_or()?;
    p.ws();
    if p.i < p.b.len() {
        return Err(p.err("unexpected trailing input"));
    }
    Ok(Compiled {
        expr: e,
        variables: p.variables,
    })
}

impl P<'_> {
    fn err(&self, m: &str) -> XPathError {
        error_at(m, self.i)
    }

    fn parse_or(&mut self) -> Result<Expr, XPathError> {
        // The single entry point of the recursive-descent chain, so
        // one check here bounds every path back into it. `depth` is
        // the number of sub-expressions already open around this one:
        // the top-level expression is at depth 0, and `max_depth`
        // nested parentheses, predicates or arguments are accepted.
        if self.depth > self.max_depth {
            return Err(self.err("expression nested too deeply"));
        }
        self.depth += 1;
        let result = self.parse_binary(0);
        self.depth -= 1;
        result
    }

    /// Count one operator, step, predicate or call against the budget.
    fn charge(&mut self) -> Result<(), XPathError> {
        self.ops += 1;
        if self.ops > self.max_ops {
            return Err(self.err("expression has too many operators"));
        }
        Ok(())
    }

    fn parse_union(&mut self) -> Result<Expr, XPathError> {
        let first = self.parse_path_expr()?;
        let mut rest = Vec::new();
        while self.eat("|") {
            self.charge()?;
            rest.push((BinaryOp::Union, self.parse_path_expr()?));
        }
        Ok(chain(first, rest))
    }

    /// `PathExpr`: a location path, or a primary expression with any
    /// predicates and a continuing path (`FilterExpr`, section 3.3).
    ///
    /// Kept to two calls so that its frame, which is on the stack for
    /// every level of nesting, stays small.
    fn parse_path_expr(&mut self) -> Result<Expr, XPathError> {
        match self.try_primary()? {
            Some(primary) => self.parse_filter_tail(primary),
            None => self.parse_path(),
        }
    }

    /// A primary expression, if one is next; `None` leaves the input
    /// where it was for a location path.
    ///
    /// Which of the two is decided by the first token. `.` is the
    /// ambiguous one: `.5` is a number, `.` and `..` are steps, so a
    /// dot commits to a number only when a digit follows it.
    fn try_primary(&mut self) -> Result<Option<Expr>, XPathError> {
        match self.peek() {
            Some(b'(') => self.parse_parenthesised().map(Some),
            Some(b'\'' | b'"') => self.parse_literal().map(Some),
            Some(b'$') => self.parse_variable().map(Some),
            Some(c) if c.is_ascii_digit() => self.parse_number().map(Some),
            Some(b'.')
                if self.b.get(self.i + 1).is_some_and(u8::is_ascii_digit) =>
            {
                self.parse_number().map(Some)
            }
            _ => self.try_function_call(),
        }
    }

    fn parse_parenthesised(&mut self) -> Result<Expr, XPathError> {
        self.i += 1;
        let e = self.parse_or()?;
        if !self.eat(")") {
            return Err(self.err("expected )"));
        }
        Ok(e)
    }

    /// The predicates and continuing path that may follow a primary
    /// expression. A primary followed by neither is itself.
    fn parse_filter_tail(&mut self, base: Expr) -> Result<Expr, XPathError> {
        let predicates = self.parse_predicates()?;
        let mut steps = Vec::new();
        match self.eat_separator() {
            Some(Separator::Double) => {
                steps.push(self.descendant_step());
                self.parse_relative_path(&mut steps)?;
            }
            Some(Separator::Single) => self.parse_relative_path(&mut steps)?,
            None => {}
        }
        if predicates.is_empty() && steps.is_empty() {
            return Ok(base);
        }
        Ok(Expr::Filter(Box::new(Filter {
            base,
            predicates,
            steps,
        })))
    }

    /// `$name` or `$prefix:name`, resolved like a name test.
    fn parse_variable(&mut self) -> Result<Expr, XPathError> {
        self.i += 1;
        let at = self.i;
        let Some(name) = self.try_name() else {
            return Err(error_at("expected a variable name after `$`", at));
        };
        let resolved = match name.split_once(':') {
            Some((prefix, local)) => {
                ExpandedName::qualified(self.resolve_prefix(prefix)?, local)
            }
            None => ExpandedName::local(name),
        };
        if !self.variables.contains(&resolved) {
            self.variables.push(resolved.clone());
        }
        Ok(Expr::Variable(Box::new(resolved)))
    }

    fn parse_literal(&mut self) -> Result<Expr, XPathError> {
        let at = self.i;
        self.try_literal()
            .map(Expr::Literal)
            .ok_or_else(|| error_at("unterminated string literal", at))
    }

    fn parse_number(&mut self) -> Result<Expr, XPathError> {
        let start = self.i;
        while self.i < self.b.len()
            && (self.b[self.i].is_ascii_digit() || self.b[self.i] == b'.')
        {
            self.i += 1;
        }
        self.s[start..self.i]
            .parse::<f64>()
            .map(Expr::Number)
            .map_err(|_| self.err("invalid number"))
    }

    /// A function call, if one is next: a name that is not a node
    /// type, followed by `(`. Anything else is left for the path
    /// parser, with the input rewound.
    ///
    /// An unknown name is refused here rather than left to evaluate
    /// to an empty node-set. Returning empty makes a misspelling
    /// indistinguishable from a document that genuinely has no match
    /// -- the caller gets a wrong answer with nothing attached to say
    /// so. Both this and the arity check report at the name's offset,
    /// which is where an editor should underline.
    fn try_function_call(&mut self) -> Result<Option<Expr>, XPathError> {
        self.ws();
        let at = self.i;
        let Some(name) = self.try_name() else {
            return Ok(None);
        };
        if self.peek() != Some(b'(') || is_node_type(&name) {
            self.i = at;
            return Ok(None);
        }
        let Some(func) = Func::from_name(&name) else {
            return Err(error_at("unknown function", at));
        };
        self.i += 1;
        self.charge()?;
        let args = self.parse_arguments()?;
        let (min, max) = func.arity();
        if args.len() < min || args.len() > max {
            return Err(error_at("wrong number of arguments", at));
        }
        Ok(Some(Expr::Function { func, args }))
    }

    /// The comma-separated arguments of a call, through the closing
    /// parenthesis. The opening one has been consumed.
    fn parse_arguments(&mut self) -> Result<Vec<Expr>, XPathError> {
        let mut args = Vec::new();
        if self.peek() != Some(b')') {
            loop {
                args.push(self.parse_or()?);
                if !self.eat(",") {
                    break;
                }
            }
        }
        if !self.eat(")") {
            return Err(self.err("expected ) after arguments"));
        }
        Ok(args)
    }

    fn parse_path(&mut self) -> Result<Expr, XPathError> {
        let leading = self.eat_separator();
        let absolute = leading.is_some();
        let mut steps = Vec::new();
        if leading == Some(Separator::Double) {
            steps.push(self.descendant_step());
        } else if absolute && self.at_path_end() {
            // A lone `/` selects the root and has no steps.
            return Ok(Expr::Path { absolute, steps });
        }
        // The loop is written out here rather than shared with
        // `parse_relative_path`: this function is on the recursion
        // path of a nested predicate (`.[.[.[...]]]`), and one more
        // frame per level is one less level that fits on a small
        // stack before `Limits::max_xpath_depth` can refuse the next.
        loop {
            steps.push(self.parse_step()?);
            match self.eat_separator() {
                Some(Separator::Double) => steps.push(self.descendant_step()),
                Some(Separator::Single) => {}
                None => return Ok(Expr::Path { absolute, steps }),
            }
        }
    }

    /// `RelativeLocationPath`: steps separated by `/` or `//`,
    /// appended to `steps`.
    fn parse_relative_path(
        &mut self,
        steps: &mut Vec<Step>,
    ) -> Result<(), XPathError> {
        loop {
            steps.push(self.parse_step()?);
            match self.eat_separator() {
                Some(Separator::Double) => steps.push(self.descendant_step()),
                Some(Separator::Single) => {}
                None => return Ok(()),
            }
        }
    }

    /// `//` or `/`, if one is next. The double form is tried first,
    /// or it lexes as two singles.
    fn eat_separator(&mut self) -> Option<Separator> {
        if self.eat("//") {
            Some(Separator::Double)
        } else if self.eat("/") {
            Some(Separator::Single)
        } else {
            None
        }
    }

    fn at_path_end(&mut self) -> bool {
        match self.peek() {
            None => true,
            Some(c) => {
                !(c.is_ascii_alphanumeric()
                    || matches!(c, b'_' | b'*' | b'@' | b'.' | b':'))
            }
        }
    }

    /// The `descendant-or-self::node()` step that `//` abbreviates.
    ///
    /// Counted against the budget but not checked here: `//` is
    /// always followed by a step, whose own [`P::charge`] reports the
    /// overrun one token later.
    fn descendant_step(&mut self) -> Step {
        self.ops += 1;
        Step {
            axis: Axis::DescendantOrSelf,
            test: NodeTest::Any,
            predicates: Vec::new(),
        }
    }

    fn parse_step(&mut self) -> Result<Step, XPathError> {
        self.charge()?;
        self.ws();
        if let Some(axis) = self.eat_abbreviated_step() {
            return Ok(Step {
                axis,
                test: NodeTest::Any,
                predicates: self.parse_predicates()?,
            });
        }
        let axis = self.parse_axis()?;
        let test = self.parse_node_test()?;
        let predicates = self.parse_predicates()?;
        Ok(Step {
            axis,
            test,
            predicates,
        })
    }

    fn parse_predicates(&mut self) -> Result<Vec<Expr>, XPathError> {
        let mut out = Vec::new();
        while self.eat("[") {
            self.charge()?;
            out.push(self.parse_or()?);
            if !self.eat("]") {
                return Err(self.err("expected ]"));
            }
        }
        Ok(out)
    }
}

fn error_at(m: &str, offset: usize) -> XPathError {
    XPathError {
        message: m.to_owned(),
        offset,
    }
}

/// The node-type tests that take `()` and are therefore not calls.
fn is_node_type(name: &str) -> bool {
    matches!(name, "text" | "comment" | "node" | "processing-instruction")
}

/// A step separator: `/`, or the `//` that abbreviates
/// `/descendant-or-self::node()/`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Separator {
    Single,
    Double,
}
