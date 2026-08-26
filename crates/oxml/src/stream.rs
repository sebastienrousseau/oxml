// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading a document as a sequence of events, without building a
//! tree.
//!
//! [`parse`] builds a tree, which is the right trade when you are
//! going to query it. When you are not — when you want the third
//! `<price>` and nothing else, or you are converting a document to
//! something else on the way past — the tree is a cost with no
//! return.
//!
//! # What this saves, and what it does not
//!
//! Measured on the 16,004-node document in the allocation tests,
//! reading holds **191,957 bytes at peak against the tree's
//! 2,277,184** — 92% less.
//!
//! It does **not** let you read a document larger than memory.
//! [`Reader::new`] takes a `&str`, and normalising line endings copies
//! it once more, so nearly all of that 191,957 bytes *is* the
//! document. What is removed is everything that outlives the event
//! that produced it: the arena, the interned names, the node table.
//! Reading incrementally from a `BufRead` is a separate piece of work;
//! until it exists, `quick-xml` is the right tool for input larger
//! than memory.
//!
//! [`Reader`] runs **the same scanner** [`parse`] does. That is the
//! design constraint, not an implementation detail: two XML scanners
//! in one crate would drift, and the one with fewer users would be the
//! one that was wrong. Everything below `parse_element_inner` — start
//! tags, attribute values, entity expansion, comments, processing
//! instructions, `CDATA`, character references, name rules — is shared
//! verbatim. What is not shared is the arena.
//!
//! # Examples
//!
//! ```
//! use oxml::stream::{Event, Reader};
//!
//! let mut reader = Reader::new("<a><b>text</b></a>")?;
//! let mut names = Vec::new();
//! while let Some(event) = reader.next_event()? {
//!     if let Event::StartElement { name, .. } = event {
//!         names.push(name.local);
//!     }
//! }
//! assert_eq!(names, ["a", "b"]);
//! # Ok::<(), oxml::Error>(())
//! ```
//!
//! [`parse`]: crate::parse

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, ErrorKind, Result};
use crate::parser::Parser;
use crate::tree::{Attribute, ExpandedName};
use crate::{Document, Limits};

/// One thing found in a document.
///
/// Owned rather than borrowed. A borrowing event would tie the caller
/// to the reader between calls, which is the opposite of what a
/// streaming interface is for; and text has been through entity
/// expansion and line-ending normalisation, so it frequently is not a
/// slice of the input in any case.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// An element opened.
    ///
    /// A self-closing element yields this and then
    /// [`Event::EndElement`], so a caller counting depth need not
    /// treat it specially.
    StartElement {
        /// The element's expanded name.
        name: ExpandedName,
        /// Its attributes, with names resolved and values expanded.
        attributes: Vec<(ExpandedName, String)>,
    },
    /// An element closed.
    EndElement {
        /// The element's expanded name.
        name: ExpandedName,
    },
    /// Character data, with entities expanded and `CDATA` merged in.
    ///
    /// One event per run: adjacent text, `CDATA` and references
    /// arrive together, exactly as they would become one text node.
    Text(String),
    /// A comment's content, without the `<!--` and `-->`.
    Comment(String),
    /// A processing instruction.
    ProcessingInstruction {
        /// The target, e.g. `xml-stylesheet`.
        target: String,
        /// Everything after the target, verbatim.
        data: String,
    },
}

/// A pull reader over a document.
///
/// Call [`Reader::next_event`] until it returns `None`. Errors are the
/// same [`Error`] values [`crate::parse`] produces, at the same
/// offsets, because the same scanner produced them.
#[derive(Debug)]
pub struct Reader {
    /// The document, line-ending normalised.
    ///
    /// Owned because normalisation may rewrite it, and a reader that
    /// borrowed the caller's string could not hold the rewritten form.
    text: String,
    /// Parser state, moved into a `Parser` for each event and back out
    /// again.
    ///
    /// The alternative is a self-referential struct: the `Parser`
    /// borrows `text`, which this owns. Held as a plain field rather
    /// than an `Option`, so there is no "taken" state to handle that
    /// cannot occur.
    carried: Carried,
    /// Where the scan has reached, and what is open around it.
    cursor: Cursor,
    /// Set once the document is finished or has failed.
    done: bool,
    /// Held back after a self-closing tag.
    pending_end: Option<Event>,
}

/// Where the scan has reached in the document.
///
/// Separate from [`Carried`] because the two are borrowed together
/// while `text` is borrowed immutably, and three disjoint field
/// borrows are what let the scan run without moving the reader into
/// and out of an `Option`.
#[derive(Debug)]
struct Cursor {
    /// Scanner position, carried between events.
    pos: usize,
    /// Names of the elements currently open, innermost last.
    open: Vec<(String, ExpandedName)>,
    /// Whether the prolog has been skipped.
    started: bool,
    /// Whether the one permitted root element has been seen.
    seen_root: bool,
}

/// Parser state that outlives a single event.
#[derive(Debug)]
struct Carried {
    document: Document,
    namespaces: crate::parser::Namespaces,
    names: alloc::collections::BTreeMap<String, Vec<u32>>,
    version: crate::parser::Version,
    limits: Limits,
    depth: usize,
    /// Declarations from the internal subset.
    ///
    /// Carried because the `DOCTYPE` is consumed by the first event
    /// and its entities are referenced by later ones. When each scan
    /// built a parser with `dtd: None`, every DTD-declared entity was
    /// unknown to the reader and known to `parse`.
    dtd: Option<crate::dtd::Dtd>,
    /// What is left of the document's entity-expansion budget.
    ///
    /// Per document, not per event. Rebuilding it each scan handed a
    /// bomb split across fifty text nodes fifty full budgets.
    entity_budget: usize,
}

impl Reader {
    /// Read `input` as a sequence of events.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the declaration is malformed or the input
    /// contains a character XML forbids — the checks [`crate::parse`]
    /// makes before scanning anything.
    pub fn new(input: &str) -> Result<Self> {
        Self::with_limits(input, Limits::default())
    }

    /// Read `input` under explicit resource bounds.
    ///
    /// # Errors
    ///
    /// As [`Reader::new`].
    pub fn with_limits(input: &str, limits: Limits) -> Result<Self> {
        let (text, version) = crate::parser::prepare(input, limits)?;
        Ok(Self {
            text,
            carried: Carried {
                document: Document::with_capacity(0),
                namespaces: crate::parser::Namespaces::default(),
                names: alloc::collections::BTreeMap::new(),
                version,
                limits,
                depth: 0,
                dtd: None,
                entity_budget: limits.max_entity_expansion,
            },
            cursor: Cursor {
                pos: 0,
                open: Vec::new(),
                started: false,
                seen_root: false,
            },
            done: false,
            pending_end: None,
        })
    }

    /// The next event, or `None` at the end of the document.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for the same malformed input [`crate::parse`]
    /// rejects, at the same offset.
    #[allow(clippy::should_implement_trait)] // `next` returns a Result
    pub fn next_event(&mut self) -> Result<Option<Event>> {
        if let Some(end) = self.pending_end.take() {
            return Ok(Some(end));
        }
        if self.done {
            return Ok(None);
        }
        match Self::scan(&self.text, &mut self.carried, &mut self.cursor) {
            Ok(Scanned::Event(event)) => Ok(Some(event)),
            Ok(Scanned::SelfClosed(start, end)) => {
                self.pending_end = Some(end);
                Ok(Some(start))
            }
            Ok(Scanned::Eof) => {
                self.done = true;
                Ok(None)
            }
            Err(e) => {
                // A failed reader stays failed rather than resuming
                // mid-document from a position the error left behind.
                self.done = true;
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_lines)] // one arm per construct
    fn scan(
        text: &str,
        carried: &mut Carried,
        cursor: &mut Cursor,
    ) -> Result<Scanned> {
        let mut parser = Parser {
            input: text,
            bytes: text.as_bytes(),
            pos: cursor.pos,
            doc: core::mem::replace(
                &mut carried.document,
                Document::with_capacity(0),
            ),
            name_index: core::mem::take(&mut carried.names),
            external: &crate::external::NoExternal,
            ns: core::mem::take(&mut carried.namespaces),
            depth: carried.depth,
            limits: carried.limits,
            dtd: carried.dtd.take(),
            version: carried.version,
            entity_budget: carried.entity_budget,
        };

        let result = Self::one_event(
            &mut parser,
            &mut cursor.open,
            cursor.started,
            &mut cursor.seen_root,
        );
        cursor.started = true;
        cursor.pos = parser.pos;
        carried.depth = parser.depth;
        carried.document =
            core::mem::replace(&mut parser.doc, Document::with_capacity(0));
        carried.names = core::mem::take(&mut parser.name_index);
        carried.namespaces = core::mem::take(&mut parser.ns);
        carried.dtd = parser.dtd.take();
        carried.entity_budget = parser.entity_budget;
        result
    }

    /// Read whichever construct comes next.
    ///
    /// The dispatch mirrors `parse_children` exactly, arm for arm, so
    /// the two agree on what a document contains.
    /// Reads one event from outside the root element.
    ///
    /// This is the `prolog`/`Misc*` grammar rather than element
    /// content: no character data, no second root, and a `DOCTYPE`
    /// only before the root and only once. The tree parser enforces
    /// the same rules in its document loop.
    fn outside_root(
        parser: &mut Parser<'_>,
        open: &mut Vec<(String, ExpandedName)>,
        seen_root: &mut bool,
    ) -> Result<Scanned> {
        loop {
            parser.skip_whitespace();
            if parser.pos >= parser.bytes.len() {
                if !*seen_root {
                    // A document with no element in it is not one.
                    return Err(Error::new(
                        ErrorKind::NoRootElement,
                        parser.pos,
                    ));
                }
                return Ok(Scanned::Eof);
            }
            if !parser.peek_is(b'<') {
                // Non-whitespace character data outside the root
                // element is never well-formed.
                return Err(Error::new(ErrorKind::TrailingContent, parser.pos));
            }
            if parser.starts_with("<!--") {
                return Ok(Scanned::Event(Event::Comment(
                    parser.parse_comment()?,
                )));
            }
            if parser.starts_with("<?") {
                let (target, data) = parser.parse_pi()?;
                return Ok(Scanned::Event(Event::ProcessingInstruction {
                    target,
                    data,
                }));
            }
            if parser.starts_with("<!DOCTYPE") {
                // Reachable here as well as from `skip_prolog`, because
                // a comment or PI may precede the declaration.
                if *seen_root || parser.dtd.is_some() {
                    return Err(Error::new(
                        ErrorKind::TrailingContent,
                        parser.pos,
                    ));
                }
                parser.skip_doctype()?;
                continue;
            }
            if parser.starts_with("</") {
                let name = parser.peek_end_tag_name();
                return Err(Error::new(
                    ErrorKind::UnexpectedEndTag(name),
                    parser.pos,
                ));
            }
            if *seen_root {
                return Err(Error::new(ErrorKind::TrailingContent, parser.pos));
            }
            *seen_root = true;
            return Self::start_tag(parser, open);
        }
    }

    /// Consumes one character-data run into a single `Text` event.
    ///
    /// Text, `CDATA` sections and references in any order are all
    /// character data, so they belong to one run and end at the first
    /// construct that is not -- a tag, comment or processing
    /// instruction. The tree parser accumulates exactly this run into
    /// one text node, so yielding it as one event is what makes the
    /// two agree on `a &amp; <![CDATA[b]]> c`.
    fn char_data(parser: &mut Parser<'_>) -> Result<Event> {
        let mut text = String::new();
        loop {
            if parser.starts_with("<![CDATA[") {
                parser.parse_cdata(&mut text)?;
            } else if parser.pos < parser.bytes.len() && !parser.peek_is(b'<') {
                parser.parse_text_run(&mut text)?;
            } else {
                break;
            }
        }
        // The same limit the tree parser applies when it flushes a run
        // into a text node. Accepting `Limits` and then not applying
        // them would be worse than not accepting them.
        if parser
            .limits
            .max_text_length
            .is_some_and(|m| text.len() > m)
        {
            return Err(Error::new(ErrorKind::TextTooLong, parser.pos));
        }
        Ok(Event::Text(text))
    }

    fn one_event(
        parser: &mut Parser<'_>,
        open: &mut Vec<(String, ExpandedName)>,
        started: bool,
        seen_root: &mut bool,
    ) -> Result<Scanned> {
        if !started {
            parser.skip_prolog()?;
        }

        // Outside the root element the grammar is not element content
        // but `Misc*`: comments, processing instructions and
        // whitespace, around exactly one element. Without this split
        // the reader accepted `<a/><b/>` -- two roots -- which the
        // tree parser rejects.
        if open.is_empty() {
            return Self::outside_root(parser, open, seen_root);
        }

        if parser.pos >= parser.bytes.len() {
            // The same error the tree parser gives for a document that
            // ends inside an element.
            return Err(Error::new(ErrorKind::UnexpectedEof, parser.pos));
        }

        if !parser.peek_is(b'<') || parser.starts_with("<![CDATA[") {
            return Ok(Scanned::Event(Self::char_data(parser)?));
        }

        if parser.starts_with("</") {
            let start = parser.pos;
            parser.pos += 2;
            let close = parser.parse_name()?;
            parser.skip_whitespace();
            if !parser.peek_is(b'>') {
                return Err(Error::new(
                    ErrorKind::Unterminated("end tag"),
                    start,
                ));
            }
            parser.pos += 1;
            // `open` is non-empty: an end tag at top level was
            // reported by `outside_root` before reaching here.
            let (expected, name) = open.pop().expect("inside an element");
            if close != expected {
                return Err(Error::new(
                    ErrorKind::MismatchedEndTag {
                        expected,
                        found: close.to_owned(),
                    },
                    start,
                ));
            }
            parser.ns.pop_scope();
            parser.depth -= 1;
            return Ok(Scanned::Event(Event::EndElement { name }));
        }

        if parser.starts_with("<!--") {
            return Ok(Scanned::Event(Event::Comment(parser.parse_comment()?)));
        }
        if parser.starts_with("<?") {
            let (target, data) = parser.parse_pi()?;
            return Ok(Scanned::Event(Event::ProcessingInstruction {
                target,
                data,
            }));
        }

        Self::start_tag(parser, open)
    }

    /// A start tag, and the end event a self-closing one implies.
    fn start_tag(
        parser: &mut Parser<'_>,
        open: &mut Vec<(String, ExpandedName)>,
    ) -> Result<Scanned> {
        // Before scanning, not after. `parse_element` checks depth on
        // entry, so for a malformed tag at the limit the tree parser
        // reports the depth and the reader reported whatever the scan
        // tripped over first -- found by fuzzing, at 980 bytes in.
        if parser.depth >= parser.limits.max_depth {
            return Err(Error::new(ErrorKind::DepthLimitExceeded, parser.pos));
        }

        let tag = parser.scan_start_tag()?;
        let at = tag.tag_start;

        let mut attributes = Vec::with_capacity(tag.raw_attrs.len());
        for (raw, value) in tag.raw_attrs {
            if raw == "xmlns" || raw.starts_with("xmlns:") {
                continue;
            }
            let id = parser.intern_qname(raw, false, at)?;
            let name = parser
                .doc
                .name(id)
                .cloned()
                .unwrap_or_else(|| ExpandedName::local(raw));
            // Compared by expanded name, as the tree parser does:
            // Namespaces in XML forbids two attributes with one
            // expanded name however they are spelled.
            if attributes
                .iter()
                .any(|(seen, _): &(ExpandedName, _)| *seen == name)
            {
                return Err(Error::new(
                    ErrorKind::DuplicateAttribute(raw.to_owned()),
                    at,
                ));
            }
            attributes.push((name, value));
        }

        // `tag.declared` is not used here. `scan_start_tag` has already
        // pushed the scope and bound the prefixes, so the declarations
        // have had their effect; what the tree parser does with them
        // afterwards is create namespace *nodes*, which exist for
        // XPath's `namespace::` axis and have no meaning in an event.
        // Events carry names already resolved instead.
        let id = parser.intern_qname(tag.qname, true, at)?;
        let name = parser
            .doc
            .name(id)
            .cloned()
            .unwrap_or_else(|| ExpandedName::local(tag.qname));

        let start = Event::StartElement {
            name: name.clone(),
            attributes,
        };
        if tag.self_closing {
            parser.ns.pop_scope();
            return Ok(Scanned::SelfClosed(start, Event::EndElement { name }));
        }
        parser.depth += 1;
        open.push((tag.qname.to_owned(), name));
        Ok(Scanned::Event(start))
    }
}

/// What one turn of the scanner produced.
enum Scanned {
    Event(Event),
    /// A self-closing tag, which is two events.
    SelfClosed(Event, Event),
    Eof,
}

/// Unused, but keeps `Attribute` referenced for the doc link above.
const _: Option<Attribute> = None;
