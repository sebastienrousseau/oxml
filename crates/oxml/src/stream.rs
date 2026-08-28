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
//! reading holds **191,967 bytes at peak against the tree's
//! 1,809,822** — 89% less.
//!
//! That was 92% before the document began owning its input. The gap
//! narrowed because the *tree* got cheaper, not because reading got
//! dearer: text nodes and attribute values are now ranges into the
//! input rather than strings of their own.
//!
//! It is **not faster**. Reading a 129 KB document as events takes
//! about 1.9 times as long as parsing it into a tree: an event owns
//! its text, so a name and an attribute value are copied out where
//! the tree keeps a range into the input it already holds. Measured
//! at 2.75 allocations per event against 0.50 per *node* for a parse.
//! This trades time for memory, and the trade is only worth making
//! when the memory matters.
//!
//! [`Reader::new`] does **not** let you read a document larger than
//! memory: it takes a `&str`, and normalising line endings copies it
//! once more, so nearly all of that 191,967 bytes *is* the document.
//! What it removes is everything that outlives the event that
//! produced it -- the arena, the interned names, the node table.
//!
//! [`Reader::from_reader`] does. It keeps the construct it is reading
//! and drops what it has passed, so the memory is bounded by the
//! largest single construct rather than by the document. Measured on
//! a 185 KB document and a 1,929 KB one -- ten times the input --
//! both held **34,722 bytes**, the same figure to the byte.
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
#[cfg(feature = "std")]
use std::io::{BufRead, Read as _};

use crate::error::{Error, ErrorKind, Result};
use crate::parser::Parser;
#[cfg(feature = "std")]
use crate::parser::Version;
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
    /// Where the rest of the document comes from, if anywhere.
    ///
    /// Only meaningful with `std`: without a reader to refill from,
    /// the input is always complete and there is nothing to
    /// distinguish.
    #[cfg(feature = "std")]
    source: Source,
    /// Bytes dropped off the front of `text` by compaction.
    ///
    /// Added back to every error offset, so a caller's caret lands in
    /// their document rather than in a window of it.
    consumed: usize,
}

/// Where a reader gets its text.
///
/// `Debug` is written rather than derived: the incremental variant
/// owns a `dyn BufRead`, which has no `Debug` of its own and would
/// otherwise take the whole `Reader` down with it.
#[cfg(feature = "std")]
enum Source {
    /// All of it is already in `text`.
    Complete,
    /// More may arrive.
    Incremental(Box<Incoming>),
}

/// A byte source part-way through being read.
#[cfg(feature = "std")]
struct Incoming {
    /// The caller's reader.
    inner: Box<dyn BufRead>,
    /// Whether it has returned zero bytes.
    at_eof: bool,
    /// The document's version, which decides both what counts as a
    /// line ending and which characters are legal.
    version: Version,
    /// Bytes that did not form a whole character.
    ///
    /// A read can stop in the middle of a multi-byte character, and
    /// the remainder arrives next time. Decoding without holding them
    /// back turns a legal document into an encoding error at a
    /// position that depends on the buffer size.
    partial: Vec<u8>,
    /// A trailing `\r` held back.
    ///
    /// `\r\n` normalises to one `\n`, and the pair can straddle a
    /// read. Emitting the `\r` immediately would produce two line
    /// endings where the document has one -- a difference that
    /// appears only at certain buffer sizes, which is the worst kind.
    pending_cr: bool,
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
    /// Whether the document declared `standalone="yes"`.
    standalone: bool,
    /// What is left of the document's entity-expansion budget.
    ///
    /// Per document, not per event. Rebuilding it each scan handed a
    /// bomb split across fifty text nodes fifty full budgets.
    entity_budget: usize,
}

#[cfg(feature = "std")]
impl core::fmt::Debug for Source {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Complete => f.write_str("Complete"),
            #[cfg(feature = "std")]
            Self::Incremental(incoming) => f
                .debug_struct("Incremental")
                .field("at_eof", &incoming.at_eof)
                .finish_non_exhaustive(),
        }
    }
}

#[cfg(feature = "std")]
impl Incoming {
    /// Read one buffer's worth, normalise it, and append it.
    ///
    /// Returns whether anything was added. Three things have to
    /// survive a boundary that can fall anywhere:
    ///
    /// - a multi-byte character split across two reads, held in
    ///   `partial` until the rest arrives;
    /// - a `\r\n` pair split across two reads, held as `pending_cr`
    ///   so it normalises to one `\n` rather than two;
    /// - the offset, which is the caller's and not this buffer's.
    ///
    /// Each of those, got wrong, produces a document that reads
    /// correctly at one buffer size and not another.
    fn pull(&mut self, text: &mut String, version: Version) -> Result<bool> {
        if self.at_eof && self.partial.is_empty() && !self.pending_cr {
            return Ok(false);
        }

        let mut buf = [0u8; 8192];
        let read = match self.inner.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                return Err(Error::new(
                    ErrorKind::Io(e.to_string()),
                    text.len(),
                ));
            }
        };
        if read == 0 {
            self.at_eof = true;
            // A held-back `\r` at the end of the document is a line
            // ending of its own.
            if self.pending_cr {
                self.pending_cr = false;
                text.push('\n');
                return Ok(true);
            }
            if !self.partial.is_empty() {
                return Err(Error::new(
                    ErrorKind::MalformedEncoding,
                    text.len(),
                ));
            }
            return Ok(false);
        }

        let mut bytes = core::mem::take(&mut self.partial);
        bytes.extend_from_slice(&buf[..read]);

        // Decode as much as is whole; keep the tail for next time.
        let chunk = match core::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(e) => {
                let valid = e.valid_up_to();
                if e.error_len().is_some() {
                    return Err(Error::new(
                        ErrorKind::MalformedEncoding,
                        text.len() + valid,
                    ));
                }
                // Incomplete rather than invalid: the rest is coming.
                self.partial = bytes[valid..].to_vec();
                match core::str::from_utf8(&bytes[..valid]) {
                    Ok(s) => s,
                    Err(_) => {
                        return Err(Error::new(
                            ErrorKind::MalformedEncoding,
                            text.len(),
                        ));
                    }
                }
            }
        };
        let chunk = chunk.to_owned();
        if !self.partial.is_empty() && chunk.is_empty() {
            // Nothing decodable yet; ask again.
            return Ok(true);
        }

        let before = text.len();
        // A `\r` held back from last time pairs with a `\n` now.
        let mut rest = chunk.as_str();
        if self.pending_cr {
            self.pending_cr = false;
            text.push('\n');
            if let Some(stripped) = rest.strip_prefix('\n') {
                rest = stripped;
            }
        }
        // Hold back a trailing `\r`: its partner may be in the next
        // read.
        let (body, hold) = match rest.strip_suffix('\r') {
            Some(body) if !self.at_eof => (body, true),
            _ => (rest, false),
        };
        self.pending_cr = hold;

        let normalised = crate::parser::normalize_line_endings(body, version);
        text.push_str(&normalised);
        crate::parser::check_characters(&text[before..], version, before)?;
        Ok(true)
    }
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
        let standalone = crate::parser::declared_standalone(&text);
        Ok(Self {
            text,
            carried: Carried {
                document: Document::placeholder(),
                namespaces: crate::parser::Namespaces::default(),
                names: alloc::collections::BTreeMap::new(),
                version,
                limits,
                depth: 0,
                dtd: None,
                entity_budget: limits.max_entity_expansion,
                standalone,
            },
            cursor: Cursor {
                pos: 0,
                open: Vec::new(),
                started: false,
                seen_root: false,
            },
            done: false,
            pending_end: None,
            #[cfg(feature = "std")]
            source: Source::Complete,
            consumed: 0,
        })
    }

    /// Read a document from a byte source, a buffer at a time.
    ///
    /// Where [`Reader::new`] is handed the whole document,
    /// this holds only what it needs to produce the next event: the
    /// text of the construct being read, plus whatever is still open
    /// around it. A document larger than memory is readable, which is
    /// the thing a `&str` entry point cannot do however little it
    /// retains.
    ///
    /// The events are the same events. `Reader::new` on the same bytes
    /// produces an identical sequence, and a test holds the two to it
    /// across buffer sizes down to one byte -- because a bug here
    /// looks like a document that parses at one buffer size and not
    /// another, which is nearly impossible to find from a report.
    ///
    /// # Errors
    ///
    /// As [`Reader::new`], plus any error the reader itself gives.
    /// Offsets are into the document, not into the current buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxml::stream::{Event, Reader};
    ///
    /// let xml = "<catalogue><item>Tea</item></catalogue>";
    /// let mut reader = Reader::from_reader(xml.as_bytes())?;
    /// let mut items = 0;
    /// while let Some(event) = reader.next_event()? {
    ///     if let Event::StartElement { name, .. } = event {
    ///         if name.local == "item" {
    ///             items += 1;
    ///         }
    ///     }
    /// }
    /// assert_eq!(items, 1);
    /// # Ok::<(), oxml::Error>(())
    /// ```
    #[cfg(feature = "std")]
    pub fn from_reader<R: BufRead + 'static>(reader: R) -> Result<Self> {
        Self::from_reader_with(reader, Limits::default())
    }

    /// [`Reader::from_reader`], with limits.
    ///
    /// # Errors
    ///
    /// As [`Reader::from_reader`].
    #[cfg(feature = "std")]
    pub fn from_reader_with<R: BufRead + 'static>(
        reader: R,
        limits: Limits,
    ) -> Result<Self> {
        let mut incoming = Incoming {
            inner: Box::new(reader),
            at_eof: false,
            // Provisional: the declaration decides it, and the
            // declaration is at the front of what the first read
            // returns.
            version: Version::V10,
            partial: Vec::new(),
            pending_cr: false,
        };

        // Enough for any XML declaration, so the version is known
        // before a single character is judged by it. `\u{85}` and
        // `\u{2028}` are line endings in 1.1 and ordinary characters
        // in 1.0, so reading even one character requires knowing which
        // document this is.
        let mut text = String::new();
        while text.len() < 1024 && !incoming.at_eof {
            if !incoming.pull(&mut text, Version::V10)? {
                break;
            }
        }
        let version = crate::parser::declared_version(&text)?;
        incoming.version = version;
        crate::parser::check_prolog_shape(&text)?;
        // The prefix was normalised as 1.0 while the version was
        // unknown. For a 1.1 document that is wrong, so it is redone
        // now that the answer is in hand.
        if version != Version::V10 {
            text = crate::parser::normalize_line_endings(&text, version)
                .into_owned();
        }
        crate::parser::check_characters(&text, version, 0)?;
        let standalone = crate::parser::declared_standalone(&text);

        Ok(Self {
            text,
            carried: Carried {
                document: Document::placeholder(),
                namespaces: crate::parser::Namespaces::default(),
                names: alloc::collections::BTreeMap::new(),
                version,
                limits,
                depth: 0,
                dtd: None,
                entity_budget: limits.max_entity_expansion,
                standalone,
            },
            cursor: Cursor {
                pos: 0,
                open: Vec::new(),
                started: false,
                seen_root: false,
            },
            done: false,
            pending_end: None,
            source: Source::Incremental(Box::new(incoming)),
            consumed: 0,
        })
    }

    /// Buffer until the next construct is whole, or the source ends.
    ///
    /// The scanner is never asked to work on a partial construct.
    /// Speculatively scanning and retrying on failure would be
    /// simpler to write and wrong: a half-scanned start tag has
    /// already interned a name and pushed a namespace scope, so the
    /// retry would see state the first attempt left behind.
    ///
    /// Whether a construct is whole is a question about delimiters
    /// only -- `-->`, `]]>`, `?>`, `>` -- which is cheap to answer and
    /// does not require understanding what is between them.
    #[cfg(feature = "std")]
    fn ensure_construct(&mut self) -> Result<()> {
        loop {
            if self.construct_is_whole() {
                return Ok(());
            }
            let Source::Incremental(incoming) = &mut self.source else {
                return Ok(());
            };
            let version = incoming.version;
            // Nothing more is coming: let the scanner meet the end and
            // report it, rather than inventing an error here.
            if !incoming.pull(&mut self.text, version)? {
                return Ok(());
            }
        }
    }

    /// Whether everything up to the end of the next construct is
    /// buffered.
    #[cfg(feature = "std")]
    fn construct_is_whole(&self) -> bool {
        let rest = &self.text[self.cursor.pos.min(self.text.len())..];
        if rest.is_empty() {
            return false;
        }
        if !rest.starts_with('<') {
            // Character data runs to the next `<`. The whole run has
            // to be here, because the tree parser merges a run into
            // one text node and the events must agree with it.
            return rest.contains('<');
        }
        for (opener, closer) in
            [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")]
        {
            if let Some(body) = rest.strip_prefix(opener) {
                return body.contains(closer);
            }
        }
        if rest.starts_with("<!DOCTYPE") {
            // An internal subset contains `>` freely, so the end is
            // the first `>` at bracket depth zero.
            let mut depth = 0usize;
            for c in rest.chars() {
                match c {
                    '[' => depth += 1,
                    ']' => depth = depth.saturating_sub(1),
                    '>' if depth == 0 => return true,
                    _ => {}
                }
            }
            return false;
        }
        // A tag. `>` inside an attribute value is data, so quotes are
        // tracked rather than searching for the first one.
        let mut quote: Option<char> = None;
        for c in rest.chars() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (None, '"' | '\'') => quote = Some(c),
                (None, '>') => return true,
                // Inside a quoted value everything is data; outside
                // one, anything else is part of the name or an
                // attribute.
                _ => {}
            }
        }
        false
    }

    /// Drop text the scanner has passed.
    ///
    /// This is what makes a document larger than memory readable: the
    /// buffer holds the construct being read and nothing before it.
    /// Offsets stay the caller's because `consumed` records what was
    /// dropped and every error adds it back.
    #[cfg(feature = "std")]
    fn compact(&mut self) {
        /// Below this, dropping costs more than it saves.
        const THRESHOLD: usize = 8192;

        if !matches!(self.source, Source::Incremental(_))
            || self.cursor.pos < THRESHOLD
        {
            return;
        }
        let _ = self.text.drain(..self.cursor.pos);
        self.consumed += self.cursor.pos;
        self.cursor.pos = 0;
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
        #[cfg(feature = "std")]
        self.ensure_construct()?;

        let outcome =
            Self::scan(&self.text, &mut self.carried, &mut self.cursor);
        #[cfg(feature = "std")]
        self.compact();

        // Offsets are the caller's, not this buffer's.
        let outcome = outcome.map_err(|mut e| {
            e.offset += self.consumed;
            e
        });

        match outcome {
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
                Document::placeholder(),
            ),
            name_index: core::mem::take(&mut carried.names),
            external: &crate::external::NoExternal,
            ns: core::mem::take(&mut carried.namespaces),
            depth: carried.depth,
            limits: carried.limits,
            dtd: carried.dtd.take(),
            version: carried.version,
            entity_budget: carried.entity_budget,
            entity_depth: 0,
            standalone: carried.standalone,
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
            core::mem::replace(&mut parser.doc, Document::placeholder());
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
                let c = parser.parse_comment()?;
                return Ok(Scanned::Event(Event::Comment(
                    parser.owned(c).to_owned(),
                )));
            }
            if parser.starts_with("<?") {
                let (target, data) = parser.parse_pi()?;
                return Ok(Scanned::Event(Event::ProcessingInstruction {
                    target: parser.owned(target).to_owned(),
                    data: parser.owned(data).to_owned(),
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
        let mut run = crate::parser::Run::default();
        loop {
            if parser.starts_with("<![CDATA[") {
                parser.parse_cdata(&mut run)?;
            } else if parser.pos < parser.bytes.len() && !parser.peek_is(b'<') {
                parser.parse_text_run(&mut run)?;
            } else {
                break;
            }
        }
        // The same limit the tree parser applies when it flushes a run
        // into a text node. Accepting `Limits` and then not applying
        // them would be worse than not accepting them.
        if parser.limits.max_text_length.is_some_and(|m| run.len() > m) {
            return Err(Error::new(ErrorKind::TextTooLong, parser.pos));
        }
        // An event owns its text: a borrowed one would tie the caller
        // to the reader between calls, which is the opposite of what a
        // streaming interface is for. The tree keeps the range; the
        // reader pays one copy per run and keeps nothing.
        Ok(Event::Text(run.as_str(parser.input).to_owned()))
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
            let c = parser.parse_comment()?;
            return Ok(Scanned::Event(Event::Comment(
                parser.owned(c).to_owned(),
            )));
        }
        if parser.starts_with("<?") {
            let (target, data) = parser.parse_pi()?;
            return Ok(Scanned::Event(Event::ProcessingInstruction {
                target: parser.owned(target).to_owned(),
                data: parser.owned(data).to_owned(),
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
            attributes.push((name, value.as_str(parser.input).to_owned()));
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
