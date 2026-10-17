# Security Policy

## Supported versions

oxml is pre-1.0. Only the most recent `0.0.x` release receives security
fixes. The whole suite (oxml, oxml-cli, oxml-wasm, oxml-mcp, oxml-lsp,
xmlschema, oxml-json) ships at one version, so a fix lands in all of
them at once.

## Reporting a vulnerability

Please report privately, not as a public issue:

- GitHub's [private vulnerability reporting](https://github.com/sebastienrousseau/oxml/security/advisories/new)
- or email <sebastian.rousseau@gmail.com>

You should get an acknowledgement within 72 hours and a fix or a
written assessment within 90 days. If the report is valid, you will be
credited in the advisory unless you ask not to be.

## Threat model

The full model, with what is refused, what is bounded and what is
not protected against, is [doc/SECURITY-MODEL.md](doc/SECURITY-MODEL.md).
The short version:

**Assumed:** the document is hostile. **Not assumed:** the caller is.

**XXE.** The crate contains no code that opens a file or a socket. An
external entity is parsed, because the declaration is part of the
grammar, and expands to nothing unless the caller supplies its text
through `ExternalSource`. The parser never fetches anything itself.

**Entity expansion.** Internal entities are expanded, as XML requires,
under a per-document byte budget and a nesting ceiling
(`Limits::max_entity_expansion`, `Limits::max_entity_depth`). Parameter
entities have a separate per-document expansion budget.

**Stack exhaustion.** Nesting depth is bounded by `Limits::max_depth`
(default 256), chosen against the stack a debug test thread actually
has. `XPath` expressions are bounded by `Limits::max_xpath_depth` and
`Limits::max_xpath_operators`, plus byte, syntax-node and argument limits.
DTD nesting is bounded separately by `Limits::max_dtd_depth`.

**Memory and CPU.** Attribute count and size, name length, node count,
text size, DTD size and parameter-entity expansion are each bounded by
a field of `Limits`. Default source and decoded document sizes are
unbounded; `Limits::strict()` caps each at 8 MiB. Untrusted XPath
evaluation additionally requires `EvalContext::with_limits`; legacy
evaluation remains unbounded. Host concurrency and stack budgets are
the caller's responsibility.

**Memory safety.** `#![forbid(unsafe_code)]`, checked by CI, so the
crate contains no unsafe blocks. Dependencies and the Rust runtime
remain part of the trust boundary. Safe Rust does not mean bounded
Rust, which is why the limits above exist.

## What is not a vulnerability

- Rejecting a document other parsers accept, when the document is not
  well-formed. Please still report it; it is a bug, just not a
  security one.
- Running out of memory or time under `Limits::permissive()`. That
  profile exists for documents you produced yourself.
