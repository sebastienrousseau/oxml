// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The asynchronous reader: a loop around [`Core`] and a
//! `tokio::io::AsyncBufRead`.

use tokio::io::AsyncBufReadExt as _;

use super::event::{BorrowedEvent, Event};
use super::machine::Core;
use crate::Limits;
use crate::error::{Error, ErrorKind, Result};
use crate::external::ExternalSource;
#[cfg(feature = "std")]
use std::io;

/// How much a source is asked for at a time; see the blocking reader.
const CHUNK: usize = 8192;

/// A pull reader over an asynchronous byte source.
///
/// Reads a document from a [`tokio::io::AsyncBufRead`] without loading
/// it whole or building a tree. The events, the errors and the memory
/// bound are those of [`Reader::from_reader`]: both are loops around
/// the same [`Core`].
///
/// The future [`AsyncReader::next_event`] returns is `Send` when `R`
/// is, so a reader can be driven from a spawned task.
///
/// [`Reader::from_reader`]: super::Reader::from_reader
pub struct AsyncReader<'e, R> {
    core: Core<'e>,
    inner: R,
}

impl<R> core::fmt::Debug for AsyncReader<'_, R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AsyncReader")
            .field("core", &self.core)
            .finish_non_exhaustive()
    }
}

impl<'e, R: tokio::io::AsyncBufRead + Unpin> AsyncReader<'e, R> {
    /// Read a document from an asynchronous byte source under the
    /// default limits.
    ///
    /// # Errors
    ///
    /// As [`Reader::from_reader`](super::Reader::from_reader).
    ///
    /// # Examples
    ///
    /// ```
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), oxml::Error> {
    /// use oxml::stream::{AsyncReader, Event};
    ///
    /// let xml = "<catalog><item>Book</item></catalog>";
    /// let mut reader = AsyncReader::from_reader(xml.as_bytes()).await?;
    /// let mut items = Vec::new();
    /// while let Some(event) = reader.next_event().await? {
    ///     if let Event::StartElement { name, .. } = event {
    ///         items.push(name.local);
    ///     }
    /// }
    /// assert_eq!(items, ["catalog", "item"]);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_reader(reader: R) -> Result<Self> {
        Self::from_reader_with(reader, Limits::default()).await
    }

    /// Read a document from an asynchronous byte source under explicit
    /// resource bounds.
    ///
    /// Reads as far as the XML declaration before returning, so a
    /// malformed declaration or an unsupported encoding is reported
    /// here rather than at the first event.
    ///
    /// # Errors
    ///
    /// As [`Self::from_reader`].
    pub async fn from_reader_with(reader: R, limits: Limits) -> Result<Self> {
        let mut reader = Self {
            core: Core::with_limits(limits),
            inner: reader,
        };
        while !reader.core.settled() && reader.core.needs_input() {
            reader.fill().await?;
        }
        Ok(reader)
    }

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
    /// Returns [`Error`] for malformed input, at the offset
    /// [`crate::parse`] reports, and [`ErrorKind::Io`] for an error
    /// the source gives.
    pub async fn next_event(&mut self) -> Result<Option<Event>> {
        loop {
            while self.core.needs_input() {
                self.fill().await?;
            }
            self.core.prepare()?;
            if !self.core.needs_input() {
                break;
            }
        }
        // Entered after the last await: a span held across one makes
        // the future `!Send`, and `tokio::spawn` refuses it.
        #[cfg(feature = "tracing")]
        let _span =
            tracing::trace_span!("oxml.stream.async_next_event").entered();
        Ok(self.core.next_event()?.map(BorrowedEvent::into_owned))
    }

    /// The document offset of the next construct: where the scanner
    /// will resume, and where an error in what follows is reported.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.core.offset()
    }

    /// Feed the core once more, or tell it the source has ended.
    async fn fill(&mut self) -> Result<()> {
        loop {
            let outcome = match self.inner.fill_buf().await {
                Ok([]) => self.core.feed_eof(),
                Ok(buf) => {
                    let take = buf.len().min(CHUNK);
                    let fed = self.core.feed(&buf[..take]);
                    self.inner.consume(take);
                    fed
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(e) => {
                    self.core.fail();
                    Err(Error::new(
                        ErrorKind::Io(e.to_string()),
                        self.core.end_offset(),
                    ))
                }
            };
            return outcome;
        }
    }
}
