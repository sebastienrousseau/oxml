<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Migrating from `libxml` (libxml2 bindings)

## Read this first

If you use **XSLT** or **XSD validation today**, do not migrate. oxml
has no XSLT and is not planning any; `xmlschema` exists but is early.
libxml2 remains the right answer for both.

What a move buys is the removal of a C dependency: no build toolchain
requirement, no linking, no libxml2 CVE stream, and memory safety
guaranteed by the compiler rather than by review.

## What changes

| libxml2 / `libxml` | oxml |
|---|---|
| `Parser::default().parse_string(s)` | `oxml::parse(s)` |
| `doc.get_root_element()` | `doc.root_element()` |
| `node.get_name()` | `doc.element_name(id).unwrap().local` |
| `node.get_attribute("x")` | `doc.attribute(id, "x")` |
| `node.get_child_nodes()` | `doc.children(id)` |
| `node.get_content()` | `doc.text(id)` |
| `Context::new(&doc)?.evaluate("…")` | `XPath::compile("…")?.evaluate(&doc)` |
| `XmlSchemaValidationContext` | `xmlschema` crate |
| `XsltStylesheet` | Not available |
| `doc.to_string()` | Not available |

## Parser options become `Limits`

libxml2 has a large set of parser option bits — `XML_PARSE_NOENT`,
`XML_PARSE_DTDLOAD`, `XML_PARSE_NONET`, `XML_PARSE_HUGE`. oxml has
`Limits`, and two of those options do not exist because the behaviour
they disable is not implemented:

| libxml2 option | oxml |
|---|---|
| `XML_PARSE_NONET` | Always. No network code exists. |
| `XML_PARSE_NOENT` | External entities are never substituted. |
| `XML_PARSE_DTDLOAD` | The external subset is never loaded. |
| `XML_PARSE_HUGE` | `Limits::permissive()`, except depth. |
| `XML_PARSE_NOBLANKS` | Not a setting; skip whitespace text nodes. |

This is the main behavioural difference and it cuts both ways.
libxml2's XXE exposure comes from options that can be set; oxml's
immunity comes from code that does not exist. The cost is that a
document legitimately depending on an external entity loses that
content silently.

## Error handling

libxml2 accumulates errors on a context and can recover. oxml returns
`Err` at the first problem, with a byte offset:

```rust
use oxml::parse;

let input = "<a></b>";
let error = parse(input).unwrap_err();
let (line, column) = error.line_column(input);
assert_eq!((line, column), (1, 4));
```

There is no recovery mode. A parser that guesses at a malformed
document produces a tree no two implementations agree on.

## Performance expectations

libxml2 is heavily optimised C with decades of tuning. Do not assume a
move is faster. What you get is the removal of an entire class of
memory-safety bug and a much smaller dependency surface — see
[COMPARISON.md](COMPARISON.md).
