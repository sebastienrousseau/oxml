// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Compiling `XPath` text into an [`Expr`].
//!
//! A recursive-descent parser following the `XPath` 1.0 grammar's
//! precedence ladder: `or` binds loosest, then `and`, equality,
//! relational, additive, multiplicative, unary, and finally paths.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use super::ast::{Axis, BinaryOp, Expr, NodeTest, Step};

/// Why an `XPath` expression could not be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XPathError {
    /// A human-readable description.
    pub message: String,
    /// Byte offset into the expression.
    pub offset: usize,
}

/// Every function `XPath` 1.0 defines, section 4, with the number of
/// arguments each accepts as `(name, minimum, maximum)`.
///
/// The evaluator matches on these names; the parser checks against the
/// same list so an expression naming anything else, or passing the
/// wrong number of arguments, fails to compile instead of quietly
/// evaluating to something plausible. There are no extension
/// functions, so the two sets are the same by construction.
///
/// Arity matters as much as the name. A missing argument used to read
/// as an empty string, so `starts-with("abc")` answered **true** --
/// every string starts with the empty string -- and
/// `translate("abc", "ab")` silently deleted the characters it had no
/// replacement for. Both are wrong answers that look like real ones.
const FUNCTIONS: &[(&str, usize, usize)] = &[
    // Node-set
    ("last", 0, 0),
    ("position", 0, 0),
    ("count", 1, 1),
    ("id", 1, 1),
    ("local-name", 0, 1),
    ("namespace-uri", 0, 1),
    ("name", 0, 1),
    // String
    ("string", 0, 1),
    // `concat` is the one variadic function: two arguments or more.
    ("concat", 2, usize::MAX),
    ("starts-with", 2, 2),
    ("contains", 2, 2),
    ("substring-before", 2, 2),
    ("substring-after", 2, 2),
    ("substring", 2, 3),
    ("string-length", 0, 1),
    ("normalize-space", 0, 1),
    ("translate", 3, 3),
    // Boolean
    ("boolean", 1, 1),
    ("not", 1, 1),
    ("true", 0, 0),
    ("false", 0, 0),
    ("lang", 1, 1),
    // Number
    ("number", 0, 1),
    ("sum", 1, 1),
    ("floor", 1, 1),
    ("ceiling", 1, 1),
    ("round", 1, 1),
];

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
    /// crate. Bounded by [`crate::MAX_DEPTH`].
    depth: usize,
    /// Prefix-to-URI bindings for name tests.
    ///
    /// `XPath` 1.0 resolves a prefix against the *expression's* context,
    /// not the document's declarations -- the same prefix can mean
    /// different things in the two, and resolving against the document
    /// would make an expression's meaning depend on which document it
    /// ran against.
    namespaces: &'a [(&'a str, &'a str)],
}

/// Compile an `XPath` 1.0 expression.
///
/// # Errors
///
/// Returns [`XPathError`] if the expression is malformed.
pub fn compile(expr: &str) -> Result<Expr, XPathError> {
    compile_with(expr, &[])
}

/// Compile with namespace prefixes bound to URIs.
///
/// # Errors
///
/// Returns [`XPathError`] if the expression is malformed, or uses a
/// prefix that `namespaces` does not bind.
pub fn compile_with(
    expr: &str,
    namespaces: &[(&str, &str)],
) -> Result<Expr, XPathError> {
    let mut p = P {
        s: expr,
        b: expr.as_bytes(),
        i: 0,
        depth: 0,
        namespaces,
    };
    let e = p.parse_or()?;
    p.ws();
    if p.i < p.b.len() {
        return Err(p.err("unexpected trailing input"));
    }
    Ok(e)
}

impl P<'_> {
    fn err(&self, m: &str) -> XPathError {
        XPathError {
            message: m.to_owned(),
            offset: self.i,
        }
    }

    fn ws(&mut self) {
        while self.i < self.b.len()
            && matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n')
        {
            self.i += 1;
        }
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(tok) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    /// Match a word operator, ensuring it is not the prefix of a name.
    ///
    /// Without the boundary check, `andover` would lex as `and` +
    /// `over`, and a path step named `divide` would become a division.
    fn eat_word(&mut self, w: &str) -> bool {
        self.ws();
        let rest = &self.s[self.i..];
        if let Some(after) = rest.strip_prefix(w) {
            if after.chars().next().is_none_or(|c| !is_name_char(c)) {
                self.i += w.len();
                return true;
            }
        }
        false
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.b.get(self.i).copied()
    }

    fn parse_or(&mut self) -> Result<Expr, XPathError> {
        // The single entry point of the recursive-descent chain, so
        // one check here bounds every path back into it.
        if self.depth >= crate::MAX_DEPTH {
            return Err(self.err("expression nested too deeply"));
        }
        self.depth += 1;
        let result = self.parse_or_inner();
        self.depth -= 1;
        result
    }

    fn parse_or_inner(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.parse_and()?;
        while self.eat_word("or") {
            let rhs = self.parse_and()?;
            lhs = bin(BinaryOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.parse_equality()?;
        while self.eat_word("and") {
            let rhs = self.parse_equality()?;
            lhs = bin(BinaryOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.parse_relational()?;
        loop {
            let op = if self.eat("!=") {
                BinaryOp::Ne
            } else if self.eat("=") {
                BinaryOp::Eq
            } else {
                break;
            };
            let rhs = self.parse_relational()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.parse_additive()?;
        loop {
            // `<=` before `<`, or the longer operator never matches.
            let op = if self.eat("<=") {
                BinaryOp::Le
            } else if self.eat(">=") {
                BinaryOp::Ge
            } else if self.eat("<") {
                BinaryOp::Lt
            } else if self.eat(">") {
                BinaryOp::Gt
            } else {
                break;
            };
            let rhs = self.parse_additive()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = if self.eat("+") {
                BinaryOp::Add
            } else if self.eat("-") {
                BinaryOp::Sub
            } else {
                break;
            };
            let rhs = self.parse_multiplicative()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = if self.eat("*") {
                BinaryOp::Mul
            } else if self.eat_word("div") {
                BinaryOp::Div
            } else if self.eat_word("mod") {
                BinaryOp::Mod
            } else {
                break;
            };
            let rhs = self.parse_unary()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, XPathError> {
        if self.eat("-") {
            let e = self.parse_unary()?;
            return Ok(Expr::Negate(Box::new(e)));
        }
        self.parse_union()
    }

    fn parse_union(&mut self) -> Result<Expr, XPathError> {
        let mut lhs = self.parse_primary()?;
        while self.eat("|") {
            let rhs = self.parse_primary()?;
            lhs = bin(BinaryOp::Union, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_primary(&mut self) -> Result<Expr, XPathError> {
        self.ws();
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let e = self.parse_or()?;
                if !self.eat(")") {
                    return Err(self.err("expected )"));
                }
                Ok(e)
            }
            Some(b'\'' | b'"') => self.parse_literal(),
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                // `.` is ambiguous: `.5` is a number, `.` and `..` are
                // steps. Only commit to a number if a digit follows.
                if c == b'.'
                    && !self.b.get(self.i + 1).is_some_and(u8::is_ascii_digit)
                {
                    self.parse_path()
                } else {
                    self.parse_number()
                }
            }
            _ => self.parse_path_or_function(),
        }
    }

    /// A quoted string, if one is next. Used by
    /// `processing-instruction('target')`, the only node test that
    /// takes an argument.
    fn try_literal(&mut self) -> Option<String> {
        let quote = *self.b.get(self.i)?;
        if quote != b'\'' && quote != b'"' {
            return None;
        }
        self.i += 1;
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i] != quote {
            self.i += 1;
        }
        if self.i >= self.b.len() {
            return None;
        }
        let v = self.s[start..self.i].to_owned();
        self.i += 1;
        Some(v)
    }

    fn parse_literal(&mut self) -> Result<Expr, XPathError> {
        let quote = self.b[self.i];
        self.i += 1;
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i] != quote {
            self.i += 1;
        }
        if self.i >= self.b.len() {
            return Err(self.err("unterminated string literal"));
        }
        let v = self.s[start..self.i].to_owned();
        self.i += 1;
        Ok(Expr::Literal(v))
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

    fn parse_path_or_function(&mut self) -> Result<Expr, XPathError> {
        let save = self.i;
        if let Some(name) = self.try_name() {
            self.ws();
            if self.peek() == Some(b'(')
                && !matches!(
                    name.as_str(),
                    "text" | "comment" | "node" | "processing-instruction"
                )
            {
                self.i += 1;
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
                // An unknown name is rejected here rather than left to
                // evaluate to an empty node-set. Returning empty makes
                // a misspelling indistinguishable from a document that
                // genuinely has no match -- the caller gets a wrong
                // answer with nothing attached to say so.
                let Some(&(_, min, max)) =
                    FUNCTIONS.iter().find(|(f, _, _)| *f == name)
                else {
                    return Err(self.err("unknown function"));
                };
                if args.len() < min || args.len() > max {
                    return Err(self.err("wrong number of arguments"));
                }
                return Ok(Expr::Function { name, args });
            }
        }
        self.i = save;
        self.parse_path()
    }

    fn parse_path(&mut self) -> Result<Expr, XPathError> {
        self.ws();
        let mut absolute = false;
        let mut steps = Vec::new();

        if self.s[self.i..].starts_with("//") {
            self.i += 2;
            absolute = true;
            steps.push(Step {
                axis: Axis::DescendantOrSelf,
                test: NodeTest::Any,
                predicates: Vec::new(),
            });
        } else if self.peek() == Some(b'/') {
            self.i += 1;
            absolute = true;
            // A lone `/` selects the root and has no steps.
            if self.at_path_end() {
                return Ok(Expr::Path { absolute, steps });
            }
        }

        loop {
            steps.push(self.parse_step()?);
            self.ws();
            if self.s[self.i..].starts_with("//") {
                self.i += 2;
                steps.push(Step {
                    axis: Axis::DescendantOrSelf,
                    test: NodeTest::Any,
                    predicates: Vec::new(),
                });
            } else if self.peek() == Some(b'/') {
                self.i += 1;
            } else {
                break;
            }
        }
        Ok(Expr::Path { absolute, steps })
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

    fn parse_step(&mut self) -> Result<Step, XPathError> {
        self.ws();
        // Abbreviations first: `..` before `.`, or `..` lexes as two.
        if self.s[self.i..].starts_with("..") {
            self.i += 2;
            return Ok(Step {
                axis: Axis::Parent,
                test: NodeTest::Any,
                predicates: self.parse_predicates()?,
            });
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            return Ok(Step {
                axis: Axis::SelfAxis,
                test: NodeTest::Any,
                predicates: self.parse_predicates()?,
            });
        }

        let axis = if self.eat("@") {
            Axis::Attribute
        } else {
            let save = self.i;
            match self.try_name() {
                Some(n) if self.s[self.i..].starts_with("::") => {
                    self.i += 2;
                    axis_from_name(&n)
                        .ok_or_else(|| self.err("unknown axis"))?
                }
                _ => {
                    self.i = save;
                    Axis::Child
                }
            }
        };

        let test = self.parse_node_test()?;
        let predicates = self.parse_predicates()?;
        Ok(Step {
            axis,
            test,
            predicates,
        })
    }

    fn parse_node_test(&mut self) -> Result<NodeTest, XPathError> {
        self.ws();
        if self.eat("*") {
            return Ok(NodeTest::Wildcard);
        }
        let name = self
            .try_name()
            .ok_or_else(|| self.err("expected a node test"))?;
        self.ws();
        if self.peek() == Some(b'(') {
            self.i += 1;
            // `processing-instruction` is the one node test that takes
            // an argument: an optional literal naming the target.
            if name == "processing-instruction" {
                self.ws();
                let target = self.try_literal();
                self.ws();
                if !self.eat(")") {
                    return Err(self.err("expected )"));
                }
                return Ok(NodeTest::ProcessingInstruction(target));
            }
            if !self.eat(")") {
                return Err(self.err("expected ()"));
            }
            return match name.as_str() {
                "text" => Ok(NodeTest::Text),
                "comment" => Ok(NodeTest::Comment),
                "node" => Ok(NodeTest::Any),
                _ => Err(self.err("unknown node type")),
            };
        }
        match name.split_once(':') {
            Some((prefix, local)) => {
                // `xml` is bound by specification and never declared.
                let uri = if prefix == "xml" {
                    "http://www.w3.org/XML/1998/namespace"
                } else {
                    self.namespaces
                        .iter()
                        .find(|(p, _)| *p == prefix)
                        .map(|(_, uri)| *uri)
                        .ok_or_else(|| {
                            self.err(&alloc::format!(
                                "unbound namespace prefix `{prefix}`; bind it \
                                 with XPath::compile_with_namespaces"
                            ))
                        })?
                };
                Ok(NodeTest::Name {
                    namespace: Some(uri.to_owned()),
                    local: local.to_owned(),
                })
            }
            // Unprefixed: matches nodes in no namespace only.
            None => Ok(NodeTest::Name {
                namespace: None,
                local: name,
            }),
        }
    }

    fn parse_predicates(&mut self) -> Result<Vec<Expr>, XPathError> {
        let mut out = Vec::new();
        while self.eat("[") {
            out.push(self.parse_or()?);
            if !self.eat("]") {
                return Err(self.err("expected ]"));
            }
        }
        Ok(out)
    }

    fn try_name(&mut self) -> Option<String> {
        self.ws();
        let rest = &self.s[self.i..];
        let mut end = 0;
        for (idx, c) in rest.char_indices() {
            if idx == 0 {
                if !is_name_start(c) {
                    return None;
                }
            } else if !is_name_char(c) {
                break;
            }
            // A `:` that is doubled is an axis separator, not part of
            // the name. Without this the name swallows `parent::a`
            // whole, the `::` test then fails, and the step silently
            // degrades to the child axis.
            if c == ':' && rest[idx + 1..].starts_with(':') {
                break;
            }
            end = idx + c.len_utf8();
        }
        if end == 0 {
            return None;
        }
        let name = rest[..end].to_owned();
        self.i += end;
        Some(name)
    }
}

fn bin(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn axis_from_name(n: &str) -> Option<Axis> {
    Some(match n {
        "child" => Axis::Child,
        "descendant" => Axis::Descendant,
        "descendant-or-self" => Axis::DescendantOrSelf,
        "parent" => Axis::Parent,
        "ancestor" => Axis::Ancestor,
        "ancestor-or-self" => Axis::AncestorOrSelf,
        "self" => Axis::SelfAxis,
        "attribute" => Axis::Attribute,
        "following-sibling" => Axis::FollowingSibling,
        "preceding-sibling" => Axis::PrecedingSibling,
        "following" => Axis::Following,
        "preceding" => Axis::Preceding,
        "namespace" => Axis::Namespace,
        _ => return None,
    })
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')
}
