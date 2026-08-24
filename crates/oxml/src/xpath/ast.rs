// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The `XPath` expression tree.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Which direction and which nodes a step selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// `child::` — the default axis.
    Child,
    /// `descendant::`
    Descendant,
    /// `descendant-or-self::` — the `//` shorthand.
    DescendantOrSelf,
    /// `parent::`
    Parent,
    /// `ancestor::`
    Ancestor,
    /// `ancestor-or-self::`
    AncestorOrSelf,
    /// `self::` — the `.` shorthand.
    SelfAxis,
    /// `attribute::` — the `@` shorthand.
    Attribute,
    /// `following-sibling::`
    FollowingSibling,
    /// `preceding-sibling::`
    PrecedingSibling,
    /// `following::` — everything after the context node in document
    /// order that is not one of its descendants.
    Following,
    /// `preceding::` — everything before the context node in document
    /// order that is not one of its ancestors.
    Preceding,
}

/// What a step matches once the axis has produced candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeTest {
    /// `*` — any element (or any attribute on the attribute axis).
    Wildcard,
    /// A name test, resolved at compile time.
    ///
    /// `namespace` is the URI the expression's prefix was bound to, or
    /// `None` for an unprefixed name. Per `XPath` 1.0 an unprefixed name
    /// test matches only nodes in **no** namespace: the default
    /// namespace of the expression context does not apply to node
    /// tests. That is the classic surprise of `XPath` 1.0 and it is what
    /// every conforming engine does.
    Name {
        /// The namespace URI, if the name was prefixed.
        namespace: Option<String>,
        /// The local part.
        local: String,
    },
    /// `text()`
    Text,
    /// `comment()`
    Comment,
    /// `processing-instruction()`, optionally narrowed to one target
    /// as `processing-instruction('name')`.
    ProcessingInstruction(Option<String>),
    /// `node()` — anything at all.
    Any,
}

/// One location step: an axis, a test, and zero or more predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The axis to walk.
    pub axis: Axis,
    /// The test applied to each candidate.
    pub test: NodeTest,
    /// Predicates, applied left to right.
    pub predicates: Vec<Expr>,
}

/// An `XPath` expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A location path. `absolute` means it started with `/`.
    Path {
        /// Whether the path starts at the document root.
        absolute: bool,
        /// The steps, in order.
        steps: Vec<Step>,
    },
    /// A literal string.
    Literal(String),
    /// A literal number.
    Number(f64),
    /// A binary operation.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A function call.
    Function {
        /// The function's name.
        name: String,
        /// Its arguments.
        args: Vec<Expr>,
    },
    /// Unary negation.
    Negate(Box<Expr>),
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `=`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `and`
    And,
    /// `or`
    Or,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `div`
    Div,
    /// `mod`
    Mod,
    /// `|` — node-set union.
    Union,
}
