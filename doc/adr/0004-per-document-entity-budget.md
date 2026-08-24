# 0004 — Charge entity expansion to the document

**Status:** Accepted

## Context

Internal entity expansion has to be bounded or the billion-laughs
attack turns 1 KB into 10⁹ characters.

The first implementation bounded each *reference*: every expansion had
to fit within an allowance.

## Decision

The budget belongs to the **document**. Every expanded character is
deducted from one allowance that lasts the whole parse.

## Consequences

A per-reference budget stops the exponential shape and misses the
quadratic one. Measured on the first implementation: a document
referencing a single 100 KB entity a thousand times, at depth one,
produced **100 MB from 100 KB of input** while every individual
expansion stayed comfortably inside its allowance.

Charging the document means the thousandth reference finds the budget
already spent.

Two bounds now apply together:

| Bound | Shape it stops |
|---|---|
| `max_entity_depth` | Exponential — entities referencing entities |
| `max_entity_expansion` | Quadratic — many references to one entity |

## The cost, stated

The default budget is 10 MB, which is generous enough that no real
document is refused — and generous enough that refusing a nine-level
billion-laughs document takes **66 ms** on a release build, from under
1 KB of input. `Limits::strict()` caps expansion at 100 KB and refuses
the same document in **25 µs**.

That factor of 2,600 is the argument for using `strict()` on anything
arriving over a network, and it is stated in the README rather than
left to be discovered under load.
