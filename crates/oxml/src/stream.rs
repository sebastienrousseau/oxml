// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading a document as a sequence of events, without building a
//! tree.
//!
//! [`parse`] builds a tree, which is the right trade when you are
//! going to query it. When you are not -- when you want the third
//! `<price>` and nothing else, or you are converting a document to
//! something else on the way past -- the tree is a cost with no
//! return.
//!
//! # Three entry points, one machine
//!
//! - [`Reader::new`] reads a `&str` already in memory.
//! - [`Reader::from_reader`] reads a `std::io::BufRead` a buffer at a
//!   time, holding only the construct in hand; a document larger than
//!   memory is readable.
//! - [`AsyncReader`] does the same over a `tokio::io::AsyncBufRead`
//!   (feature `async`).
//!
//! All three are loops around [`Core`], which does no I/O at all:
//! bytes are fed to it and events pulled from it, so any other way of
//! getting bytes -- another runtime, a ring buffer, a parser
//! combinator -- can drive it directly.
//!
//! # What this saves, and what it does not
//!
//! Measured on the 16,004-node document in the allocation tests,
//! reading holds **191,967 bytes at peak against the tree's
//! 1,809,822** -- 89% less. [`Reader::new`] does **not** let you read
//! a document larger than memory: it takes a `&str`, and normalising
//! line endings copies it once, so nearly all of that 191,967 bytes
//! *is* the document. What it removes is everything that outlives the
//! event that produced it: the arena, the interned names, the node
//! table.
//!
//! [`Reader::from_reader`] does. Measured on a 185 KB document and a
//! 1,929 KB one -- ten times the input -- both held **34,722 bytes**,
//! the same figure to the byte.
//!
//! Reading is not faster than parsing. An owned [`Event`] copies its
//! names and text out of the buffer, and [`BorrowedEvent`] copies only
//! what was rewritten -- an expanded entity, a merged `CDATA` section
//! -- but a name is resolved and owned either way. Measured with the
//! allocation harness in `tests/allocations.rs`, reading the 6,001
//! events of a 2,000-item catalogue costs about 2.6 allocations per
//! event against 0.5 per *node* for a parse. This trades time for
//! memory, and the trade is only worth making when the memory
//! matters.
//!
//! [`Reader`] runs **the same scanner** [`parse`] does. That is the
//! design constraint, not an implementation detail: two XML scanners
//! in one crate would drift, and the one with fewer users would be the
//! one that was wrong. Everything below `parse_element_inner` -- start
//! tags, attribute values, entity expansion, comments, processing
//! instructions, `CDATA`, character references, name rules, the
//! document type declaration -- is shared verbatim. What is not shared
//! is the arena.
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
//! A reader is also an iterator, which ends after the document or
//! after the first error:
//!
//! ```
//! use oxml::stream::{Event, Reader};
//!
//! let texts: Vec<String> = Reader::new("<a>one<b>two</b></a>")?
//!     .filter_map(|event| match event {
//!         Ok(Event::Text(text)) => Some(text),
//!         _ => None,
//!     })
//!     .collect();
//! assert_eq!(texts, ["one", "two"]);
//! # Ok::<(), oxml::Error>(())
//! ```
//!
//! [`parse`]: crate::parse

mod decode;
mod event;
mod machine;
mod prelude;
mod probe;
mod reader;
mod scan;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
mod async_reader;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub use async_reader::AsyncReader;
pub use event::{BorrowedEvent, Event};
pub use machine::Core;
pub use reader::{Complete, Reader, Source};
