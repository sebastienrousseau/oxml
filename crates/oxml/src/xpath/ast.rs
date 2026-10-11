// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The `XPath` expression tree.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::tree::ExpandedName;

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
    /// `namespace::` — every namespace in scope for an element, its
    /// own declarations and those inherited from its ancestors.
    Namespace,
}

impl Axis {
    /// Whether the axis runs backwards through the document.
    ///
    /// `XPath` 1.0 section 2.4: on a reverse axis the proximity
    /// position counts in *reverse* document order, so
    /// `preceding-sibling::*[1]` is the nearest sibling, not the first
    /// in the document. The four reverse axes are `ancestor`,
    /// `ancestor-or-self`, `preceding` and `preceding-sibling`;
    /// `parent` and `self` select at most one node, so direction does
    /// not arise, and the specification lists them as forward.
    #[must_use]
    pub const fn is_reverse(self) -> bool {
        matches!(
            self,
            Self::Ancestor
                | Self::AncestorOrSelf
                | Self::Preceding
                | Self::PrecedingSibling
        )
    }

    /// Every axis, in the specification's order. Thirteen.
    pub const ALL: [Self; 13] = [
        Self::Child,
        Self::Descendant,
        Self::DescendantOrSelf,
        Self::Parent,
        Self::Ancestor,
        Self::AncestorOrSelf,
        Self::SelfAxis,
        Self::Attribute,
        Self::FollowingSibling,
        Self::PrecedingSibling,
        Self::Following,
        Self::Preceding,
        Self::Namespace,
    ];

    /// The axis's name as written in an expression.
    #[must_use]
    pub const fn name(self) -> &'static str {
        // Indexed by discriminant, in the order of `ALL`.
        const NAMES: [&str; 13] = [
            "child",
            "descendant",
            "descendant-or-self",
            "parent",
            "ancestor",
            "ancestor-or-self",
            "self",
            "attribute",
            "following-sibling",
            "preceding-sibling",
            "following",
            "preceding",
            "namespace",
        ];
        NAMES[self as usize]
    }

    /// The axis with this name, if `XPath` 1.0 defines one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }
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
    /// `*:local` — any namespace, one local name.
    ///
    /// **An extension, not `XPath` 1.0.** The 1.0 grammar has no way to
    /// say "this local name in whatever namespace"; 2.0 added exactly
    /// this production. It is implemented because binding a prefix for
    /// every namespace in a deeply namespaced document is boilerplate
    /// that answers no question, and it is documented as an extension
    /// so nobody ports an expression to another 1.0 engine and finds
    /// it rejected.
    AnyNamespace {
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
    /// A left-associative chain of three or more operands.
    ///
    /// `a + b + c` is, by the grammar, `(a + b) + c`; representing it
    /// as nested [`Expr::Binary`] nodes gives a tree whose depth is
    /// the number of terms, and every recursive walk of it (evaluate,
    /// clone, compare, drop) then needs stack in proportion to the
    /// expression's length. A chain keeps the terms in one `Vec`, so
    /// depth comes only from genuine nesting (parentheses, predicates,
    /// arguments), which [`crate::Limits::max_xpath_depth`] bounds.
    ///
    /// Evaluation is the left fold `((first op1 e1) op2 e2) ...`, with
    /// exactly the semantics the nested form has: `and`/`or` stop at
    /// the first deciding operand, and `a = b = c` compares the
    /// boolean result of `a = b` against `c`. Every operator in one
    /// chain is from one precedence level, so `and` and `or` never
    /// share a chain with anything else.
    ///
    /// Two operands are an [`Expr::Binary`]; a chain always has at
    /// least two entries in `rest`.
    Chain {
        /// The leftmost operand.
        first: Box<Expr>,
        /// Each further operator and the operand to its right, in
        /// source order.
        rest: Vec<(BinaryOp, Expr)>,
    },
    /// A function call, resolved to the library function at compile
    /// time so that evaluation dispatches on an enum rather than
    /// comparing names per call.
    Function {
        /// The function.
        func: Func,
        /// Its arguments.
        args: Vec<Expr>,
    },
    /// Unary negation.
    Negate(Box<Expr>),
    /// A variable reference, `$name` or `$prefix:name`, with the
    /// prefix resolved against the expression's namespace bindings at
    /// compile time. Bound at evaluation through
    /// [`EvalContext`](super::EvalContext).
    ///
    /// Boxed, as [`Expr::Filter`] is, to keep `Expr` at 40 bytes: a
    /// larger `Expr` makes every parser frame larger, and the parser
    /// has to fit [`crate::Limits::max_xpath_depth`] levels of
    /// recursion on a small thread's stack before the limit can
    /// refuse the next.
    Variable(Box<ExpandedName>),
    /// A `FilterExpr`, optionally continued as a path: a primary
    /// expression's node-set filtered by predicates, then walked by
    /// further steps. `(//a)[1]`, `id('x')/b` and `$set//c` are all
    /// this.
    Filter(Box<Filter>),
}

/// The body of an [`Expr::Filter`].
///
/// The predicates see the node-set in document order and count
/// position forwards, as a filter is not a step and has no axis.
/// Only built when at least one predicate or step follows the
/// primary: a bare primary is itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    /// The primary expression. Must evaluate to a node-set.
    pub base: Expr,
    /// Predicates applied to the node-set, left to right.
    pub predicates: Vec<Expr>,
    /// The steps of a relative location path that follows, or empty
    /// if none does.
    pub steps: Vec<Step>,
}

/// Every function `XPath` 1.0 defines, section 4.
///
/// An expression names one of these or fails to compile: there are
/// no extension functions, so an unknown name is an error rather than
/// an empty result that is indistinguishable from a document with no
/// match. Resolving the name at compile time also means evaluation
/// dispatches on this enum rather than comparing strings per call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Func {
    /// `last()`
    Last,
    /// `position()`
    Position,
    /// `count(node-set)`
    Count,
    /// `id(object)`
    Id,
    /// `local-name(node-set?)`
    LocalName,
    /// `namespace-uri(node-set?)`
    NamespaceUri,
    /// `name(node-set?)`
    Name,
    /// `string(object?)`
    String,
    /// `concat(string, string, string*)`
    Concat,
    /// `starts-with(string, string)`
    StartsWith,
    /// `contains(string, string)`
    Contains,
    /// `substring-before(string, string)`
    SubstringBefore,
    /// `substring-after(string, string)`
    SubstringAfter,
    /// `substring(string, number, number?)`
    Substring,
    /// `string-length(string?)`
    StringLength,
    /// `normalize-space(string?)`
    NormalizeSpace,
    /// `translate(string, string, string)`
    Translate,
    /// `boolean(object)`
    Boolean,
    /// `not(boolean)`
    Not,
    /// `true()`
    True,
    /// `false()`
    False,
    /// `lang(string)`
    Lang,
    /// `number(object?)`
    Number,
    /// `sum(node-set)`
    Sum,
    /// `floor(number)`
    Floor,
    /// `ceiling(number)`
    Ceiling,
    /// `round(number)`
    Round,
}

impl Func {
    /// Every function, in the specification's order. Twenty-seven.
    pub const ALL: [Self; 27] = [
        Self::Last,
        Self::Position,
        Self::Count,
        Self::Id,
        Self::LocalName,
        Self::NamespaceUri,
        Self::Name,
        Self::String,
        Self::Concat,
        Self::StartsWith,
        Self::Contains,
        Self::SubstringBefore,
        Self::SubstringAfter,
        Self::Substring,
        Self::StringLength,
        Self::NormalizeSpace,
        Self::Translate,
        Self::Boolean,
        Self::Not,
        Self::True,
        Self::False,
        Self::Lang,
        Self::Number,
        Self::Sum,
        Self::Floor,
        Self::Ceiling,
        Self::Round,
    ];

    /// The function's name as written in an expression.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.spec().0
    }

    /// The number of arguments the function accepts, as an inclusive
    /// `(minimum, maximum)`. `concat` is the one variadic function:
    /// two or more.
    ///
    /// Arity matters as much as the name. A missing argument used to
    /// read as an empty string, so `starts-with("abc")` answered
    /// **true** (every string starts with the empty string) and
    /// `translate("abc", "ab")` silently deleted the characters it had
    /// no replacement for. Both are wrong answers that look like real
    /// ones.
    #[must_use]
    pub const fn arity(self) -> (usize, usize) {
        let (_, min, max) = self.spec();
        (min, max)
    }

    /// The function with this name, if `XPath` 1.0 defines one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    /// Name and arity, from one table so they cannot drift apart.
    const fn spec(self) -> (&'static str, usize, usize) {
        // Indexed by discriminant, in the order of `ALL`.
        const SPECS: [(&str, usize, usize); 27] = [
            ("last", 0, 0),
            ("position", 0, 0),
            ("count", 1, 1),
            ("id", 1, 1),
            ("local-name", 0, 1),
            ("namespace-uri", 0, 1),
            ("name", 0, 1),
            ("string", 0, 1),
            ("concat", 2, usize::MAX),
            ("starts-with", 2, 2),
            ("contains", 2, 2),
            ("substring-before", 2, 2),
            ("substring-after", 2, 2),
            ("substring", 2, 3),
            ("string-length", 0, 1),
            ("normalize-space", 0, 1),
            ("translate", 3, 3),
            ("boolean", 1, 1),
            ("not", 1, 1),
            ("true", 0, 0),
            ("false", 0, 0),
            ("lang", 1, 1),
            ("number", 0, 1),
            ("sum", 1, 1),
            ("floor", 1, 1),
            ("ceiling", 1, 1),
            ("round", 1, 1),
        ];
        SPECS[self as usize]
    }
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

impl BinaryOp {
    /// The operator as written in an expression.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        // Indexed by discriminant, in declaration order.
        const SYMBOLS: [&str; 14] = [
            "=", "!=", "<", "<=", ">", ">=", "and", "or", "+", "-", "*", "div",
            "mod", "|",
        ];
        SYMBOLS[self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every parser frame holds a few of these; see [`Expr::Variable`].
    #[test]
    fn expr_stays_small() {
        assert!(core::mem::size_of::<Expr>() <= 40);
    }
}
