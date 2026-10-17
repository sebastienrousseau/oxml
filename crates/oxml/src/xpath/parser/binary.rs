// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The binary and unary operators: precedence climbing over the six
//! binary levels, and the run of unary minuses.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::{P, XPathError};
use crate::xpath::ast::{BinaryOp, Expr};

/// The binary operators, with how tightly each binds: `or` loosest,
/// `* div mod` tightest. `word` marks the ones lexed as words, which
/// must not match the prefix of a name. A longer lexeme is listed
/// before its prefix (`!=` before `=`, `<=` before `<`) or the shorter
/// one always wins.
const OPERATORS: &[(&str, bool, BinaryOp, u8)] = &[
    ("or", true, BinaryOp::Or, 0),
    ("and", true, BinaryOp::And, 1),
    ("!=", false, BinaryOp::Ne, 2),
    ("=", false, BinaryOp::Eq, 2),
    ("<=", false, BinaryOp::Le, 3),
    (">=", false, BinaryOp::Ge, 3),
    ("<", false, BinaryOp::Lt, 3),
    (">", false, BinaryOp::Gt, 3),
    ("+", false, BinaryOp::Add, 4),
    ("-", false, BinaryOp::Sub, 4),
    ("*", false, BinaryOp::Mul, 5),
    ("div", true, BinaryOp::Div, 5),
    ("mod", true, BinaryOp::Mod, 5),
];

impl P<'_> {
    /// Precedence climbing over the six binary levels.
    ///
    /// `min_prec` is the loosest operator this call may consume; a
    /// right operand is parsed one level tighter, so `1 + 2 * 3` puts
    /// the product under the sum. The operators one call consumes
    /// come in non-increasing precedence (a tighter one would have
    /// been taken by the operand before it), so a run at one level is
    /// gathered flat into one [`Expr::Chain`], and a drop to a looser
    /// level first folds the run so far into that chain's first
    /// operand. Every chain therefore holds one level's operators,
    /// and `and`/`or` never share one with anything else.
    pub(super) fn parse_binary(
        &mut self,
        min_prec: u8,
    ) -> Result<Expr, XPathError> {
        let mut first = self.parse_unary()?;
        let mut rest = Vec::new();
        let mut level = u8::MAX;
        while let Some((op, prec)) = self.eat_binary_op(min_prec) {
            self.charge()?;
            if prec < level && !rest.is_empty() {
                first = chain(first, core::mem::take(&mut rest));
            }
            level = prec;
            rest.push((op, self.parse_binary(prec + 1)?));
        }
        Ok(chain(first, rest))
    }

    /// The next binary operator, if there is one and it binds at
    /// least as tightly as `min_prec`. One that binds looser is left
    /// for the caller that owns that level.
    fn eat_binary_op(&mut self, min_prec: u8) -> Option<(BinaryOp, u8)> {
        let save = self.i;
        let found = OPERATORS.iter().find(|&&(tok, word, _, _)| {
            if word {
                self.eat_word(tok)
            } else {
                self.eat(tok)
            }
        });
        match found {
            Some(&(_, _, op, prec)) if prec >= min_prec => Some((op, prec)),
            _ => {
                self.i = save;
                None
            }
        }
    }

    /// `-` repeated: counted in a loop rather than parsed recursively,
    /// so a run of a million of them is refused by the operator budget
    /// instead of overflowing the stack.
    ///
    /// Negation is exact for every `f64`, so a pair of them is the
    /// identity *on numbers*; what it still does is convert the
    /// operand, `--'3'` being the number 3 and `--'x'` NaN. One pair
    /// is kept for that reason, and the parity of the rest decides
    /// whether a single negation remains on top.
    fn parse_unary(&mut self) -> Result<Expr, XPathError> {
        self.charge_node()?;
        let mut negations = 0_usize;
        while self.eat("-") {
            self.charge()?;
            negations += 1;
        }
        let operand = self.parse_union()?;
        Ok(match negations {
            0 => operand,
            n if n % 2 == 1 => Expr::Negate(Box::new(operand)),
            _ => Expr::Negate(Box::new(Expr::Negate(Box::new(operand)))),
        })
    }
}

/// Fold one precedence level into a node: the lone operand itself, a
/// [`Expr::Binary`] for two operands, a flat [`Expr::Chain`] for more.
pub(super) fn chain(first: Expr, mut rest: Vec<(BinaryOp, Expr)>) -> Expr {
    if rest.len() > 1 {
        return Expr::Chain {
            first: Box::new(first),
            rest,
        };
    }
    match rest.pop() {
        None => first,
        Some((op, rhs)) => Expr::Binary {
            op,
            lhs: Box::new(first),
            rhs: Box::new(rhs),
        },
    }
}
