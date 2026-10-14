// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Rendering an [`Expr`] back to `XPath` 1.0 text.
//!
//! The output is the canonical unabbreviated form: every step spells
//! its axis, `//` becomes `/descendant-or-self::node()/`, `.` becomes
//! `self::node()`, and every binary operation is parenthesised, so the
//! text means the same thing whatever the reader assumes about
//! precedence. Compiling the rendering gives back the same tree, which
//! is what makes it usable as a cache key or a normal form.
//!
//! A name in a namespace needs a prefix to be written as `XPath` 1.0,
//! and the tree does not keep one: it resolves prefixes to URIs at
//! compile time, because that is what the name *means*. [`XPath`]
//! renders with the bindings it was compiled under and so writes the
//! prefix back; a bare [`Expr`] has no bindings and writes the
//! expanded name in Clark notation, `{uri}local`, which is unambiguous
//! but is not `XPath` 1.0 syntax.
//!
//! [`XPath`]: super::XPath

use core::fmt::{self, Display, Formatter, Write as _};

use super::ast::{BinaryOp, Expr, NodeTest, Step};

/// The namespace `xml` is bound to by specification.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// A renderer carrying the prefix bindings to write names with.
pub(crate) struct Render<'a> {
    /// Prefix-to-URI bindings; a URI is written with the first prefix
    /// bound to it.
    pub(crate) namespaces:
        &'a [(alloc::string::String, alloc::string::String)],
}

impl Display for Expr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Render { namespaces: &[] }.expr(self, f)
    }
}

impl Render<'_> {
    pub(crate) fn expr(&self, e: &Expr, f: &mut Formatter<'_>) -> fmt::Result {
        match e {
            Expr::Literal(s) => literal(s, f),
            Expr::Number(n) => number(*n, f),
            Expr::Negate(inner) => self.negate(inner, f),
            Expr::Variable(name) => {
                f.write_char('$')?;
                self.name(name.namespace.as_deref(), &name.local, f)
            }
            Expr::Path { absolute, steps } => self.path(*absolute, steps, f),
            compound => self.compound(compound, f),
        }
    }

    /// `-e` or `--e`. The parser reads any run of `-` as one negation
    /// or a pair of them, so a third one directly beneath a pair is
    /// parenthesised: `--(-1)`, not `---1`, which reads back as `-1`.
    fn negate(&self, inner: &Expr, f: &mut Formatter<'_>) -> fmt::Result {
        let Expr::Negate(operand) = inner else {
            f.write_char('-')?;
            return self.expr(inner, f);
        };
        f.write_str("--")?;
        if matches!(**operand, Expr::Negate(_)) {
            f.write_char('(')?;
            self.expr(operand, f)?;
            f.write_char(')')
        } else {
            self.expr(operand, f)
        }
    }

    /// The expressions with sub-expressions.
    fn compound(&self, e: &Expr, f: &mut Formatter<'_>) -> fmt::Result {
        match e {
            Expr::Binary { op, lhs, rhs } => {
                f.write_char('(')?;
                self.operand(*op, lhs, f)?;
                self.operator(*op, rhs, f)?;
                f.write_char(')')
            }
            Expr::Chain { first, rest } => self.chain(first, rest, f),
            Expr::Function { func, args } => self.call(func.name(), args, f),
            Expr::Filter(filter) => {
                self.filter(&filter.base, &filter.predicates, &filter.steps, f)
            }
            _ => unreachable!("not a compound expression"),
        }
    }

    /// `(first op e op e ...)`: one precedence level, parenthesised.
    fn chain(
        &self,
        first: &Expr,
        rest: &[(BinaryOp, Expr)],
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        f.write_char('(')?;
        match rest.first() {
            Some((op, _)) => self.operand(*op, first, f)?,
            None => self.expr(first, f)?,
        }
        for (op, e) in rest {
            self.operator(*op, e, f)?;
        }
        f.write_char(')')
    }

    /// ` op rhs`, the tail of one binary operation.
    fn operator(
        &self,
        op: BinaryOp,
        rhs: &Expr,
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        write!(f, " {} ", op.symbol())?;
        self.operand(op, rhs, f)
    }

    /// One operand of `op`. A union's operands are path expressions in
    /// the grammar and `-` binds more loosely than `|`, so a negation
    /// there is parenthesised: `-1 | 2` would read back as `-(1 | 2)`.
    fn operand(
        &self,
        op: BinaryOp,
        e: &Expr,
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        if matches!(op, BinaryOp::Union) && matches!(e, Expr::Negate(_)) {
            f.write_char('(')?;
            self.expr(e, f)?;
            f.write_char(')')
        } else {
            self.expr(e, f)
        }
    }

    fn call(
        &self,
        name: &str,
        args: &[Expr],
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(name)?;
        f.write_char('(')?;
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            self.expr(a, f)?;
        }
        f.write_char(')')
    }

    /// `(base)[pred]...` then `/step` for each step. The base is
    /// always parenthesised: it may be a path, and a path followed by
    /// a predicate would otherwise read as a predicate on its last
    /// step.
    fn filter(
        &self,
        base: &Expr,
        predicates: &[Expr],
        steps: &[Step],
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        f.write_char('(')?;
        self.expr(base, f)?;
        f.write_char(')')?;
        self.predicates(predicates, f)?;
        for step in steps {
            f.write_char('/')?;
            self.step(step, f)?;
        }
        Ok(())
    }

    fn path(
        &self,
        absolute: bool,
        steps: &[Step],
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        if absolute {
            f.write_char('/')?;
        }
        for (i, step) in steps.iter().enumerate() {
            if i > 0 {
                f.write_char('/')?;
            }
            self.step(step, f)?;
        }
        Ok(())
    }

    fn step(&self, step: &Step, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(step.axis.name())?;
        f.write_str("::")?;
        self.node_test(&step.test, f)?;
        self.predicates(&step.predicates, f)
    }

    fn predicates(&self, preds: &[Expr], f: &mut Formatter<'_>) -> fmt::Result {
        for p in preds {
            f.write_char('[')?;
            self.expr(p, f)?;
            f.write_char(']')?;
        }
        Ok(())
    }

    fn node_test(&self, test: &NodeTest, f: &mut Formatter<'_>) -> fmt::Result {
        match test {
            NodeTest::Wildcard => f.write_char('*'),
            NodeTest::Name { namespace, local } => {
                self.name(namespace.as_deref(), local, f)
            }
            NodeTest::AnyNamespace { local } => write!(f, "*:{local}"),
            kind => kind_test(kind, f),
        }
    }

    /// A name: bare when it has no namespace, `prefix:local` when a
    /// binding supplies the prefix, `{uri}local` otherwise.
    fn name(
        &self,
        namespace: Option<&str>,
        local: &str,
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        let Some(uri) = namespace else {
            return f.write_str(local);
        };
        if uri == XML_NAMESPACE {
            return write!(f, "xml:{local}");
        }
        let bound = self.namespaces.iter().find(|(_, u)| u == uri);
        match bound {
            Some((prefix, _)) => write!(f, "{prefix}:{local}"),
            None => write!(f, "{{{uri}}}{local}"),
        }
    }
}

/// `text()`, `comment()`, `processing-instruction()` with or without
/// a target, and `node()`.
fn kind_test(test: &NodeTest, f: &mut Formatter<'_>) -> fmt::Result {
    match test {
        NodeTest::Text => f.write_str("text()"),
        NodeTest::Comment => f.write_str("comment()"),
        NodeTest::ProcessingInstruction(Some(target)) => {
            f.write_str("processing-instruction(")?;
            literal(target, f)?;
            f.write_char(')')
        }
        NodeTest::ProcessingInstruction(None) => {
            f.write_str("processing-instruction()")
        }
        _ => f.write_str("node()"),
    }
}

/// A string literal in whichever quote it does not contain. A literal
/// the parser produced never contains the quote that delimited it;
/// one built by hand that contains both is written as a `concat` of
/// pieces, since `XPath` 1.0 has no escape.
fn literal(s: &str, f: &mut Formatter<'_>) -> fmt::Result {
    if !s.contains('\'') {
        return write!(f, "'{s}'");
    }
    if !s.contains('"') {
        return write!(f, "\"{s}\"");
    }
    f.write_str("concat(")?;
    for (i, piece) in s.split('\'').enumerate() {
        if i > 0 {
            f.write_str("\"'\", ")?;
        }
        write!(f, "'{piece}', ")?;
    }
    f.write_str("'')")
}

/// A number as the parser would read it back: Rust's shortest
/// round-trip form, which never uses an exponent, so `0.1` is `0.1`
/// and not `0.1000000000000000055511151231257827`. The values the
/// grammar cannot spell are written as the expressions that produce
/// them.
fn number(n: f64, f: &mut Formatter<'_>) -> fmt::Result {
    if n.is_nan() {
        return f.write_str("(0 div 0)");
    }
    if n.is_infinite() {
        return f.write_str(if n > 0.0 { "(1 div 0)" } else { "(-1 div 0)" });
    }
    if n.is_sign_negative() {
        return write!(f, "-{}", -n);
    }
    write!(f, "{n}")
}
