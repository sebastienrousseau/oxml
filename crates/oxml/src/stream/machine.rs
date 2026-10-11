// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The reader with the I/O taken out.

use alloc::string::String;
use alloc::vec::Vec;

use super::decode::Decoder;
use super::event::BorrowedEvent;
use super::prelude::{Decision, Prelude};
use super::probe::Probe;
use super::scan::{self, Carried, Cursor, Pending, Scanned};
use crate::Limits;
use crate::encoding::Encoding;
use crate::error::{Error, ErrorKind, Result};
use crate::external::ExternalSource;
use crate::parser::Version;

/// Below this, dropping passed text costs more than it saves.
const COMPACT_THRESHOLD: usize = 8192;

/// A source that supplies no external entities.
const NO_EXTERNAL: &(dyn ExternalSource + Sync) = &crate::external::NoExternal;

/// Where the bytes are on their way to being text.
#[derive(Debug)]
enum Stage {
    /// Held until the declaration can be read.
    Prelude(Prelude),
    /// Decoded as they arrive.
    Body(Decoder),
}

/// A streaming reader that does no I/O: bytes go in, events come out.
///
/// [`Reader`] and [`AsyncReader`] are loops around this, one blocking
/// and one `async`; a caller with some other way of getting bytes --
/// a different runtime, a ring buffer, a parser combinator -- drives
/// it directly. The protocol is three calls:
///
/// 1. While [`needs_input`] is true, [`feed`] more bytes, or
///    [`feed_eof`] when there are none.
/// 2. Call [`next_event`]. `Some` is an event; `None` means either
///    that the document is complete or that input is needed again,
///    and `needs_input` tells which.
/// 3. Repeat.
///
/// Bytes are decoded as they arrive -- UTF-8, UTF-16 with or without
/// a byte-order mark, and ISO-8859-1 when declared -- and their line
/// endings normalised, under the version the XML declaration names;
/// the declaration is read before anything after it is judged.
/// Memory is bounded by the largest single construct, and that in
/// turn by [`Limits::max_construct_bytes`]: text the scanner has
/// passed is dropped, and a construct that grows past the cap without
/// ending is refused rather than buffered until memory runs out.
///
/// Errors are the ones [`crate::parse_bytes`] gives, at the same
/// offsets into the decoded document, with one qualification: an
/// illegal character or a malformed byte sequence is reported when
/// its chunk is decoded, which may be after events before it have
/// been yielded, and a construct before it that is itself malformed
/// is reported first. The error's kind and offset do not depend on
/// how the bytes were chunked.
///
/// # Examples
///
/// ```
/// use oxml::stream::{BorrowedEvent, Core};
///
/// let document = b"<a><b>text</b></a>";
/// let mut core = Core::new();
/// let mut chunks = document.chunks(3);
/// let mut names = Vec::new();
/// loop {
///     if core.needs_input() {
///         match chunks.next() {
///             Some(chunk) => core.feed(chunk)?,
///             None => core.feed_eof()?,
///         }
///     } else if let Some(event) = core.next_event()? {
///         if let BorrowedEvent::StartElement { name, .. } = event {
///             names.push(name.local);
///         }
///     } else if !core.needs_input() {
///         break;
///     }
/// }
/// assert_eq!(names, ["a", "b"]);
/// # Ok::<(), oxml::Error>(())
/// ```
///
/// [`Reader`]: super::Reader
/// [`AsyncReader`]: super::AsyncReader
/// [`needs_input`]: Core::needs_input
/// [`feed`]: Core::feed
/// [`feed_eof`]: Core::feed_eof
#[derive(Clone, Copy, Debug, Default)]
struct StreamEnd {
    eof: bool,
    done: bool,
}

/// [`next_event`]: Core::next_event
pub struct Core<'e> {
    /// The document so far, decoded and line-ending normalised, less
    /// what compaction has dropped off the front.
    text: String,
    carried: Carried,
    cursor: Cursor,
    external: &'e (dyn ExternalSource + Sync),
    stage: Stage,
    /// The search for the end of the construct at the cursor.
    probe: Option<Probe>,
    /// Whether that construct has wholly arrived, or the source has
    /// ended and the scanner may meet the end itself.
    whole: bool,
    /// Stream termination status (EOF reached, reader completed or failed).
    end: StreamEnd,
    /// Whether passed text is dropped. Off for a document handed over
    /// whole, where the drain would only move the bytes about.
    compacts: bool,
    /// Held back after a self-closing tag.
    pending_end: Option<Pending>,
    /// Bytes dropped off the front of `text` by compaction. Added back
    /// to every offset, so a caller's caret lands in their document
    /// rather than in a window of it.
    consumed: usize,
}

impl core::fmt::Debug for Core<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Core")
            .field("buffered", &self.text.len())
            .field("consumed", &self.consumed)
            .field("stage", &self.stage)
            .field("whole", &self.whole)
            .field("eof", &self.end.eof)
            .field("done", &self.end.done)
            .finish_non_exhaustive()
    }
}

impl Default for Core<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'e> Core<'e> {
    /// A reader under the default [`Limits`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    /// A reader under explicit resource bounds.
    #[must_use]
    pub fn with_limits(limits: Limits) -> Self {
        Self {
            text: String::new(),
            carried: Carried::new(limits),
            cursor: Cursor::new(),
            external: NO_EXTERNAL,
            stage: Stage::Prelude(Prelude::default()),
            probe: None,
            whole: false,
            end: StreamEnd::default(),
            compacts: true,
            pending_end: None,
            consumed: 0,
        }
    }

    /// Where external entities come from, as
    /// [`crate::parse_with_external`] takes it. `Sync` so that the
    /// reader stays `Send`.
    #[must_use]
    pub fn with_external(
        mut self,
        external: &'e (dyn ExternalSource + Sync),
    ) -> Self {
        self.external = external;
        self
    }

    /// Whether the next event cannot be decided from what has been
    /// fed: the construct at the cursor has not wholly arrived and the
    /// source has not ended.
    #[must_use]
    pub fn needs_input(&self) -> bool {
        !self.end.done
            && !self.end.eof
            && self.pending_end.is_none()
            && !self.whole
    }

    /// Hand over the next bytes of the document.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the bytes are not valid in the document's
    /// encoding, contain a character XML forbids, settle a declaration
    /// that is malformed, or grow a construct past
    /// [`Limits::max_construct_bytes`]. A failed reader stays failed.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        self.guarded(|core| core.ingest(bytes))
    }

    /// The source has ended; nothing more will be fed.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the source ended inside a character, or
    /// the declaration it ended inside is malformed.
    pub fn feed_eof(&mut self) -> Result<()> {
        if self.end.done || self.end.eof {
            return Ok(());
        }
        self.end.eof = true;
        self.guarded(|core| {
            core.read_declaration()?;
            if let Stage::Body(decoder) = &mut core.stage {
                decoder.finish(&mut core.text)?;
            }
            core.after_input()
        })
    }

    /// The next event, borrowing from the buffer where it can.
    ///
    /// `None` when the document is complete, or when [`needs_input`]
    /// is true and more must be fed first.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for the same malformed input [`crate::parse`]
    /// rejects, at the same offset.
    ///
    /// [`needs_input`]: Core::needs_input
    pub fn next_event(&mut self) -> Result<Option<BorrowedEvent<'_>>> {
        if let Some(pending) = self.pending_end.take() {
            return Ok(Some(pending.into_event()));
        }
        self.prepare()?;
        if !self.ready() {
            return Ok(None);
        }
        self.compact();
        // Failed until the scan proves otherwise: a failed reader
        // stays failed rather than resuming mid-document from a
        // position the error left behind.
        self.end.done = true;
        let consumed = self.consumed;
        let external: &dyn ExternalSource = self.external;
        let scanned = scan::scan(
            &self.text,
            &mut self.carried,
            &mut self.cursor,
            external,
        )
        .map_err(|e| offset_by(e, consumed))?;
        let Some((event, pending)) = yielded(scanned) else {
            return Ok(None);
        };
        self.pending_end = pending;
        // Over the fields rather than through a method, because the
        // event still borrows the text.
        self.whole =
            whole(&mut self.probe, &self.text, &mut self.cursor, self.end.eof);
        self.end.done = false;
        Ok(Some(event))
    }

    /// Consume what yields no event at the cursor -- the declaration,
    /// a `DOCTYPE` -- while it has wholly arrived, so that afterwards
    /// [`Core::needs_input`] answers for the next *event*.
    ///
    /// Called by [`Core::next_event`], and by a driver between filling
    /// and asking: a `DOCTYPE` that was whole may be followed by a tag
    /// that is not, and the driver finds that out here rather than by
    /// getting `None` back.
    ///
    /// Not done as the bytes arrive, so that an external source given
    /// after construction is in place before the `DOCTYPE` is read.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for a malformed declaration or `DOCTYPE`,
    /// and the reader stays failed.
    pub fn prepare(&mut self) -> Result<()> {
        self.guarded(|core| {
            while core.ready() {
                core.compact();
                let external: &dyn ExternalSource = core.external;
                let skipped = scan::skip_eventless(
                    &core.text,
                    &mut core.carried,
                    &mut core.cursor,
                    external,
                )
                .map_err(|e| offset_by(e, core.consumed))?;
                if !skipped {
                    break;
                }
                core.whole = whole(
                    &mut core.probe,
                    &core.text,
                    &mut core.cursor,
                    core.end.eof,
                );
            }
            Ok(())
        })
    }

    /// Whether there is a whole construct to scan.
    fn ready(&self) -> bool {
        !self.end.done && self.whole
    }

    /// The document offset of the next construct: where the scanner
    /// will resume, which is where an error in what follows would be
    /// reported.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.consumed + self.cursor.pos
    }

    /// Take a document the tree parser's `prepare` has already
    /// normalised and checked, exactly as [`crate::parse`] would have
    /// it: nothing to decode, nothing more to read, and nothing to
    /// drop, since the drain would only move the bytes about.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the declaration or a `DOCTYPE` is
    /// malformed: the constructs consumed before the first event.
    pub(crate) fn adopt(
        &mut self,
        text: String,
        version: Version,
        standalone: bool,
    ) -> Result<()> {
        self.carried.declare(version, standalone);
        self.stage = Stage::Body(Decoder::new(Encoding::Utf8, version));
        self.text = text;
        self.end.eof = true;
        self.compacts = false;
        self.guarded(Self::after_input)
    }

    /// Whether the declaration has been read and decoding has begun.
    pub(crate) fn settled(&self) -> bool {
        matches!(self.stage, Stage::Body(_))
    }

    /// Mark the reader failed, for an error the source reported.
    #[cfg(feature = "std")]
    pub(crate) fn fail(&mut self) {
        self.end.done = true;
    }

    /// Where the next byte from the source would land.
    #[cfg(feature = "std")]
    pub(crate) fn end_offset(&self) -> usize {
        self.consumed + self.text.len()
    }

    /// Run `f`, and leave the reader failed if it fails.
    fn guarded(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        if self.end.done {
            return Ok(());
        }
        let outcome = f(self);
        if outcome.is_err() {
            self.end.done = true;
        }
        outcome
    }

    fn ingest(&mut self, bytes: &[u8]) -> Result<()> {
        match &mut self.stage {
            Stage::Body(decoder) => {
                decoder.push(bytes, &mut self.text, self.consumed)?;
            }
            Stage::Prelude(prelude) => {
                prelude.push(bytes);
                let held = prelude.len();
                self.check_cap(held, 0)?;
                self.read_declaration()?;
            }
        }
        self.after_input()
    }

    /// Read the declaration if it has wholly arrived, and decode what
    /// was held back under it.
    fn read_declaration(&mut self) -> Result<()> {
        let Stage::Prelude(prelude) = &self.stage else {
            return Ok(());
        };
        let Some(decision) = prelude.decide(self.end.eof)? else {
            return Ok(());
        };
        let held = self.begin(decision);
        let Stage::Body(decoder) = &mut self.stage else {
            return Ok(());
        };
        decoder.push(&held[decision.bom..], &mut self.text, 0)
    }

    /// Move from the prelude to the body, returning the held bytes.
    fn begin(&mut self, decision: Decision) -> Vec<u8> {
        self.carried.declare(decision.version, decision.standalone);
        let decoder = Decoder::new(decision.encoding, decision.version);
        match core::mem::replace(&mut self.stage, Stage::Body(decoder)) {
            Stage::Prelude(prelude) => prelude.into_bytes(),
            Stage::Body(_) => Vec::new(),
        }
    }

    /// After bytes arrived: is the construct at the cursor whole, and
    /// if not, has it grown past the cap?
    fn after_input(&mut self) -> Result<()> {
        if !self.settled() {
            return Ok(());
        }
        self.whole =
            whole(&mut self.probe, &self.text, &mut self.cursor, self.end.eof);
        if self.whole {
            return Ok(());
        }
        let start = self.probe.map_or(self.cursor.pos, |p| p.start);
        self.check_cap(self.text.len().saturating_sub(start), start)
    }

    /// Refuse a construct that has grown past
    /// [`Limits::max_construct_bytes`] without ending. The offset is
    /// the construct's start, in the document.
    fn check_cap(&self, buffered: usize, start: usize) -> Result<()> {
        let cap = self.carried.limits.max_construct_bytes;
        if cap.is_some_and(|max| buffered > max) {
            return Err(Error::new(
                ErrorKind::ConstructTooLarge,
                self.consumed + start,
            ));
        }
        Ok(())
    }

    /// Drop text the scanner has passed.
    ///
    /// This is what makes a document larger than memory readable: the
    /// buffer holds the construct being read and nothing before it.
    fn compact(&mut self) {
        if !self.compacts || self.cursor.pos < COMPACT_THRESHOLD {
            return;
        }
        let drained = self.cursor.pos;
        let _ = self.text.drain(..drained);
        self.consumed += drained;
        self.cursor.pos = 0;
        match &mut self.probe {
            Some(probe) if probe.start >= drained => probe.shift(drained),
            _ => self.probe = None,
        }
    }
}

/// What a scan yielded: an event and the end event it may owe, or
/// nothing at the end of the document.
fn yielded(
    scanned: Scanned<'_>,
) -> Option<(BorrowedEvent<'_>, Option<Pending>)> {
    match scanned {
        Scanned::Event(event) => Some((event, None)),
        Scanned::SelfClosed(start, pending) => Some((start, Some(pending))),
        Scanned::Eof => None,
    }
}

/// Whether the construct at the cursor has wholly arrived, resuming
/// the search where the last call left off.
///
/// Outside the root element, whitespace is skipped first: the scanner
/// would skip it anyway, and leaving the cursor before it made every
/// look at a long run of whitespace start from its beginning.
///
/// A free function over the fields rather than a method, so it can be
/// called while an event still borrows the text.
fn whole(
    probe: &mut Option<Probe>,
    text: &str,
    cursor: &mut Cursor,
    eof: bool,
) -> bool {
    if eof {
        return true;
    }
    let in_content = !cursor.open.is_empty();
    if !in_content {
        cursor.pos = skip_whitespace(text, cursor.pos);
    }
    let start = cursor.pos;
    let probe = probe.get_or_insert_with(|| Probe::at(start));
    if probe.start != start {
        *probe = Probe::at(start);
    }
    probe.is_whole(text, in_content)
}

fn skip_whitespace(text: &str, mut pos: usize) -> usize {
    let bytes = text.as_bytes();
    while pos < bytes.len()
        && matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n')
    {
        pos += 1;
    }
    pos
}

/// An error's offset, moved from the buffer into the document.
fn offset_by(mut e: Error, consumed: usize) -> Error {
    e.offset += consumed;
    e
}

/// The encoding a settled reader decodes, for tests.
#[cfg(test)]
impl Core<'_> {
    fn encoding(&self) -> Option<crate::encoding::Encoding> {
        match &self.stage {
            Stage::Body(decoder) => Some(decoder.encoding()),
            Stage::Prelude(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::Event;

    /// Every event, fed `n` bytes at a time.
    fn events(bytes: &[u8], n: usize) -> Result<Vec<Event>> {
        let mut core = Core::new();
        let mut chunks = bytes.chunks(n);
        let mut out = Vec::new();
        loop {
            while core.needs_input() {
                match chunks.next() {
                    Some(chunk) => core.feed(chunk)?,
                    None => core.feed_eof()?,
                }
            }
            if let Some(event) = core.next_event()? {
                out.push(event.into_owned());
            } else if !core.needs_input() {
                return Ok(out);
            }
        }
    }

    #[test]
    fn next_event_is_none_while_input_is_needed() {
        let mut core = Core::new();
        core.feed(b"<a><b>te").expect("fine");
        let mut seen = 0;
        while let Some(_event) = core.next_event().expect("fine") {
            seen += 1;
        }
        assert_eq!(seen, 2, "the two whole start tags");
        assert!(core.needs_input(), "the text run has not ended");
        core.feed(b"xt</b></a>").expect("fine");
        core.feed_eof().expect("fine");
        assert!(!core.needs_input());
        assert!(matches!(
            core.next_event().expect("fine"),
            Some(BorrowedEvent::Text(t)) if t == "text"
        ));
    }

    #[test]
    fn a_document_reads_the_same_whatever_the_chunking() {
        let doc =
            b"<?xml version=\"1.1\"?>\r\n<!DOCTYPE a [<!ENTITY e \"x\">]>\
                    <a x='1'>one <![CDATA[two]]> &e;\r\xC2\x85three</a>";
        let expected = events(doc, doc.len()).expect("valid");
        for n in 1..=doc.len() {
            assert_eq!(events(doc, n).expect("valid"), expected, "chunk {n}");
        }
    }

    #[test]
    fn the_declaration_picks_the_decoder() {
        let mut core = Core::new();
        core.feed(b"<?xml version='1.0' encoding='iso-8859-1'?><a/>")
            .expect("fine");
        assert_eq!(core.encoding(), Some(Encoding::Latin1));
        let mut core = Core::new();
        core.feed(b"<?x").expect("fine");
        assert_eq!(core.encoding(), None, "still deciding");
    }

    #[test]
    fn a_failed_reader_accepts_nothing_more() {
        let mut core = Core::new();
        assert!(core.feed(b"<a>\x01</a>").is_err());
        assert!(core.feed(b"<a/>").is_ok(), "ignored, not retried");
        assert!(!core.needs_input());
        assert!(core.next_event().expect("stays failed").is_none());
    }
}
