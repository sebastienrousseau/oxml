<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Architecture decision records

One file per decision that would otherwise be re-argued. Each records
what was decided, what it cost, and what would change it.

An ADR is not deleted when a decision is reversed. It is superseded,
and the new record says which one it replaces — the reasoning that
turned out to be wrong is the useful part.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-arena-over-rc-refcell.md) | An arena of indices, not `Rc<RefCell<Node>>` | Accepted |
| [0002](0002-forbid-unsafe.md) | `#![forbid(unsafe_code)]`, at a cost | Accepted |
| [0003](0003-no-external-entities.md) | Never fetch external entities | Accepted |
| [0004](0004-per-document-entity-budget.md) | Charge entity expansion to the document | Accepted |
| [0005](0005-limits-as-a-value.md) | `Limits` is a value, not a builder | Accepted |
| [0006](0006-baseline-ratchet.md) | Fail on unreviewed conformance improvement | Accepted |
| [0007](0007-owned-strings-for-now.md) | Own the strings; borrow later | Accepted |
