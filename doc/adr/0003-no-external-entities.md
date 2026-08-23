# 0003 — Never fetch external entities

**Status:** Accepted

## Context

XML lets a document declare an entity whose content lives elsewhere:

```xml
<!ENTITY xxe SYSTEM "file:///etc/passwd">
```

A parser that resolves it reads the file into the document. This is
XXE, and it is one of the most reliably exploited classes of bug in XML
tooling — usually in parsers that resolve by default, and occasionally
in parsers where the option was left on by accident.

## Decision

No code in this crate opens a file or a socket. External entities are
parsed, because the declaration is part of the grammar, and then expand
to nothing. There is no option to enable resolution.

## Consequences

**Gained**

- XXE is foreclosed structurally. There is no configuration that
  reintroduces it, so there is no configuration to get wrong.
- No I/O in the parser means no I/O errors, no timeouts, and no
  surprising blocking call inside `parse`.

**Given up**

- A document that legitimately depends on an external entity loses that
  content **silently**. It parses; the entity is empty.
- The external DTD subset is unavailable, which is what most of the
  remaining W3C conformance failures need: 163 of 164 are documents
  accepted that a parser with the external subset would reject.

## Alternatives rejected

**Resolve, with a default of off.** This is what most parsers do and it
is where the CVEs come from. A default is a property of the call site,
and call sites are copied.

**A resolver callback.** Better, and still not chosen: it puts the
security decision in the caller's hands at the moment they are least
thinking about security. It remains the likely route to supporting the
external subset — a caller supplying subsets from their own storage,
with the parser still never performing I/O.

## What would change it

Serious demand for external-subset validation. The shape would be a
caller-supplied map from identifier to content, never a fetch.
