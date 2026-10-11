// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The blocking reader: a loop around [`Core`] and a byte source.

use super::event::{BorrowedEvent, Event};
use super::machine::Core;
use crate::Limits;
use crate::error::Result;
#[cfg(feature = "std")]
use crate::error::{Error, ErrorKind};
use crate::external::ExternalSource;

/// How much a source is asked for at a time.
///
/// A `BufRead` may hold its whole input -- a `Cursor` does -- and
/// taking all of it at once would make the memory held depend on the
/// document's size rather than its largest construct.
#[cfg(feature = "std")]
const CHUNK: usize = 8192;

mod sealed {
    pub trait Sealed {}
}

/// Where a [`Reader`] gets its bytes.
///
/// Implemented for every [`std::io::BufRead`] and for [`Complete`], a
/// document handed over whole. Sealed: the set is the reader's to
/// choose, and a caller with another kind of source drives [`Core`]
/// directly.
pub trait Source: sealed::Sealed {
    /// Feed the core once more, or tell it the source has ended.
    #[doc(hidden)]
    fn fill(&mut self, core: &mut Core<'_>) -> Result<()>;
}

/// The source of a reader that was handed its whole document, as
/// [`Reader::new`] is: there is nothing more to read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Complete;

impl sealed::Sealed for Complete {}

impl Source for Complete {
    fn fill(&mut self, core: &mut Core<'_>) -> Result<()> {
        core.feed_eof()
    }
}

#[cfg(feature = "std")]
impl<R: std::io::BufRead> sealed::Sealed for R {}

#[cfg(feature = "std")]
impl<R: std::io::BufRead> Source for R {
    fn fill(&mut self, core: &mut Core<'_>) -> Result<()> {
        loop {
            let outcome = match self.fill_buf() {
                Ok([]) => core.feed_eof(),
                Ok(buf) => {
                    let take = buf.len().min(CHUNK);
                    let fed = core.feed(&buf[..take]);
                    self.consume(take);
                    fed
                }
                // The read was interrupted by a signal, not refused.
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(e) => {
                    // A source that failed is a document that cannot
                    // be finished: the reader stays failed, as it does
                    // after a parse error.
                    core.fail();
                    Err(Error::new(
                        ErrorKind::Io(e.to_string()),
                        core.end_offset(),
                    ))
                }
            };
            return outcome;
        }
    }
}

/// A pull reader over a document.
///
/// Call [`Reader::next_event`] until it returns `None`, or iterate:
/// the reader is an [`Iterator`] over `Result<Event>` that ends after
/// the document, or after the first error. Errors are the same
/// [`Error`] values [`crate::parse`] produces, at the same offsets,
/// because the same scanner produced them.
///
/// `S` is where the bytes come from: [`Complete`] for a document
/// handed over whole by [`Reader::new`], or the `BufRead` given to
/// [`Reader::from_reader`]. The reader is `Send` whenever `S` is.
/// `'e` is the lifetime of an [`ExternalSource`] given to
/// [`Reader::with_external`], and `'static` when there is none.
///
/// [`Error`]: crate::Error
pub struct Reader<'e, S = Complete> {
    core: Core<'e>,
    source: S,
}

impl<S> core::fmt::Debug for Reader<'_, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Reader")
            .field("core", &self.core)
            .finish_non_exhaustive()
    }
}

impl Reader<'_, Complete> {
    /// Read `input` as a sequence of events.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the declaration is malformed or the input
    /// contains a character XML forbids: the checks [`crate::parse`]
    /// makes before scanning anything.
    ///
    /// [`Error`]: crate::Error
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
        let mut core = Core::with_limits(limits);
        core.adopt(text, version, standalone)?;
        Ok(Self {
            core,
            source: Complete,
        })
    }
}

#[cfg(feature = "std")]
impl<R: std::io::BufRead> Reader<'_, R> {
    /// Read a document from a byte source, a buffer at a time.
    ///
    /// Where [`Reader::new`] is handed the whole document, this holds
    /// only what it needs to produce the next event: the text of the
    /// construct being read, plus whatever is still open around it. A
    /// document larger than memory is readable, which is the thing a
    /// `&str` entry point cannot do however little it retains.
    ///
    /// The events are the same events. `Reader::new` on the decoded
    /// bytes produces an identical sequence, and a test holds the two
    /// to it across buffer sizes down to one byte -- because a bug
    /// here looks like a document that parses at one buffer size and
    /// not another, which is nearly impossible to find from a report.
    ///
    /// The bytes are decoded as [`crate::parse_bytes`] decodes them:
    /// a byte-order mark first, then the declaration's `encoding`,
    /// then UTF-8. UTF-8, UTF-16 and ISO-8859-1 are read; another
    /// declared encoding is refused with
    /// [`ErrorKind::UnsupportedEncoding`].
    ///
    /// Memory is bounded by the largest single construct, and that in
    /// turn by [`Limits::max_construct_bytes`]: a source that never
    /// closes a tag, a comment or a run of text is refused once the
    /// construct passes the cap, rather than buffered until memory
    /// runs out.
    ///
    /// # Errors
    ///
    /// As [`Reader::new`], plus [`ErrorKind::Io`] for an error the
    /// source itself gives and [`ErrorKind::ConstructTooLarge`] for a
    /// construct past the cap. Offsets are into the decoded document,
    /// not into the current buffer.
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
    ///
    /// [`ErrorKind::Io`]: crate::ErrorKind::Io
    /// [`ErrorKind::ConstructTooLarge`]: crate::ErrorKind::ConstructTooLarge
    /// [`ErrorKind::UnsupportedEncoding`]: crate::ErrorKind::UnsupportedEncoding
    pub fn from_reader(reader: R) -> Result<Self> {
        Self::from_reader_with(reader, Limits::default())
    }

    /// [`Reader::from_reader`], with limits.
    ///
    /// Reads as far as the XML declaration before returning, so a
    /// malformed declaration or an unsupported encoding is reported
    /// here rather than at the first event.
    ///
    /// # Errors
    ///
    /// As [`Reader::from_reader`].
    pub fn from_reader_with(reader: R, limits: Limits) -> Result<Self> {
        let mut reader = Self {
            core: Core::with_limits(limits),
            source: reader,
        };
        while !reader.core.settled() && reader.core.needs_input() {
            reader.source.fill(&mut reader.core)?;
        }
        Ok(reader)
    }
}

impl<'e, S: Source> Reader<'e, S> {
    /// Where external entities come from, as
    /// [`crate::parse_with_external`] takes it.
    ///
    /// `Sync` so that the reader stays `Send`.
    #[must_use]
    pub fn with_external(
        mut self,
        external: &'e (dyn ExternalSource + Sync),
    ) -> Self {
        self.core = self.core.with_external(external);
        self
    }

    /// The next event, or `None` at the end of the document.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for the same malformed input [`crate::parse`]
    /// rejects, at the same offset.
    ///
    /// [`Error`]: crate::Error
    pub fn next_event(&mut self) -> Result<Option<Event>> {
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("oxml.stream.next_event").entered();

        self.next_borrowed()
            .map(|opt| opt.map(BorrowedEvent::into_owned))
    }

    /// The next event, borrowing character data and attribute values
    /// from the reader's buffer where the input holds them verbatim.
    ///
    /// # Errors
    ///
    /// As [`Reader::next_event`].
    pub fn next_borrowed(&mut self) -> Result<Option<BorrowedEvent<'_>>> {
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("oxml.stream.next_borrowed").entered();

        loop {
            while self.core.needs_input() {
                self.source.fill(&mut self.core)?;
            }
            self.core.prepare()?;
            if !self.core.needs_input() {
                break;
            }
        }
        self.core.next_event()
    }

    /// The document offset of the next construct: where the scanner
    /// will resume, and where an error in what follows is reported.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.core.offset()
    }
}

impl<S: Source> Iterator for Reader<'_, S> {
    type Item = Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_event().transpose()
    }
}
