<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Security model

What oxml refuses, what it bounds, and what it does not protect you
from. To report a vulnerability, see [SECURITY.md](../SECURITY.md).

## Contents

- [Threat model](#threat-model)
- [XXE: foreclosed by construction](#xxe-foreclosed-by-construction)
- [Entity expansion: bounded per document](#entity-expansion-bounded-per-document)
- [Stack exhaustion: bounded by depth](#stack-exhaustion-bounded-by-depth)
- [Memory: bounded, but the defaults are generous](#memory-bounded-but-the-defaults-are-generous)
- [Memory safety](#memory-safety)
- [What this does not protect you from](#what-this-does-not-protect-you-from)
- [Choosing limits](#choosing-limits)

## Threat model

**Assumed:** the document is hostile. It was written by someone who
read this file and wants to spend your CPU, your memory, or your file
descriptors.

**Not assumed:** the *caller* is hostile. A caller who sets
`Limits::permissive` and parses a 40 GB document gets what they asked
for.

## XXE: foreclosed by construction

An XML external entity attack declares an entity pointing at a local
file or a URL and references it in content, so a parser that resolves
it leaks the file into the document.

oxml contains no code that opens a file or a socket. External entities
are parsed — they must be, because the declaration is part of the
grammar — and then expand to nothing:

```rust
let src = r#"<!DOCTYPE a [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><a>&xxe;</a>"#;
let doc = oxml::parse(src).expect("well-formed: the entity is declared");
assert_eq!(doc.text(doc.root()).trim(), "");
```

This is a structural property, not a default. There is no flag to turn
resolution on, which means there is no way to configure the
vulnerability back in — the difference between a parser that is safe
and one that is safe *if you remember*.

The trade-off: a document that legitimately depends on an external
entity silently loses that content. If you need it, fetch it yourself
and pass a document with the entity already substituted.

## Entity expansion: bounded per document

The billion-laughs attack nests internal entities so that each level
multiplies the last. Ten levels of ten turns under 1 KB of input into
10⁹ characters of output.

Internal entities **are** expanded — a conforming parser must, and many
valid documents depend on it — under a budget that is charged **per
document**, not per reference.

That distinction is the whole defence. A per-reference budget bounds
each expansion and lets a *quadratic* blowup through: a document with a
thousand references to a 100 KB entity stays under any per-reference
limit and still produces 100 MB. Charging the document means the
thousandth reference finds the budget already spent.

Two bounds apply:

| Bound | What it stops |
|---|---|
| `max_entity_depth` | Exponential growth — entities referencing entities |
| `max_entity_expansion` | Total expanded bytes, across the whole document |

## Stack exhaustion: bounded by depth

Parsing descends one stack frame per open element. A document nested
deeply enough exhausts the stack, and **a stack overflow aborts the
process rather than unwinding**, so no caller can catch it. For a
service taking XML from the network, that is a denial of service.

`Limits::max_depth` defaults to 256, well above any hand-written
document.

`Limits::permissive()` deliberately leaves `max_depth` at the default
rather than raising it with the other bounds. Measured on this crate's
own test binaries, a 2 MiB thread stack overflows at around 280 levels
in a debug build, at roughly 7,489 bytes per frame. Raising the limit
to 10,000 does not buy deeper documents; it buys a crash. That was
found by setting it to 10,000 and watching the process abort.

## Memory: bounded, but the defaults are generous

`max_nodes`, `max_text_length`, `max_attribute_size`,
`max_attributes_per_element` and `max_name_length` bound how much a
document can allocate.

**The defaults leave `max_nodes` and `max_text_length` unbounded.**
That is a deliberate choice for the common case — a build tool parsing
a file it wrote — and the wrong one for a service. `Limits::strict()`
sets both.

Measured cost of the default entity budget: a nine-level billion-laughs
document is refused in **66 ms** on a release build, because 10 MB of
expansion is permitted before the budget is spent. `Limits::strict()`
refuses the same document in **25 µs**, a factor of about 2,600.

## Memory safety

`#![forbid(unsafe_code)]`, and CI greps for the attribute so that
removing it shows up in a diff rather than silently.

This rules out memory-corruption bugs categorically. It does not rule
out logic bugs, panics, or resource exhaustion, and it costs
throughput: several techniques that make the fastest parsers fast are
unavailable.

Verification beyond the type system:

- **Seven fuzz targets** (`parse`, `stream`, `tree_walk`, `mutate`,
  `parse_limits`, `xpath_compile`, `xpath_eval`) with a seeded corpus
  and an XML dictionary. `stream` checks not only that the event
  reader never panics but that it *agrees* with `parse` — same
  documents accepted, same refused with the same error at the same
  offset.
- **Miri** over the test suite, for undefined behaviour the compiler
  does not catch.
- **Property tests**, including that no input makes the parser panic
  and that error offsets always land on a character boundary.
- **The W3C conformance suite**: 2,585 tests, **zero panics**.

## What this does not protect you from

- **A caller who raises the limits.** `permissive()` exists.
- **Algorithmic cost inside XPath.** `max_xpath_depth` and
  `max_xpath_operators` bound the *expression*, not the size of the
  node-sets it builds. `//*//*//*` on a large document is expensive and
  is not refused.
- **Memory pressure from legitimate documents.** The whole tree is in
  memory. A 500 MB document needs more than 500 MB.
- **Anything after parsing.** The tree is data you asked for. If a
  document says `<script>`, oxml gives you `<script>`.

## Choosing limits

| Situation | Profile |
|---|---|
| Parsing a file you wrote, a build fixture, a test | `default()` |
| Anything arriving over a network | `strict()` |
| A one-off conversion of a machine-generated document you trust | `permissive()` |

If you take XML from the network and do nothing else from this
document, use `Limits::strict()`.
