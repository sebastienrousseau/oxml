#![no_main]
//! Reading from bytes must agree with reading from memory, however
//! the bytes are chunked.
//!
//! The first byte of the input is the chunk size; the rest is the
//! document. `Reader::from_reader` is fed that many bytes at a time
//! and held to what `Reader::new` produces on the decoded text --
//! every event, and the error that ends the read, with its offset.
//!
//! One qualification, the same one the agreement tests state: an
//! illegal character or a malformed byte sequence is found when its
//! chunk is decoded, so the streaming reader may have yielded events
//! before reporting it, and a malformed construct before it is
//! reported first, at an earlier offset. A later offset, a different
//! error, or acceptance of what decoding refused is a bug.
//!
//! "Earlier" needs one coordinate system. A decoding error is
//! reported at a byte of the input; every other error at a byte of
//! the decoded text, which in UTF-16 is a different number for the
//! same place. So an error of another kind, met before decoding
//! failed, is held to what `Reader::new` reports for the text the
//! bytes before the failure decode to.

use std::io::{BufReader, Read};

use libfuzzer_sys::fuzz_target;
use oxml::ErrorKind;
use oxml::stream::{Event, Reader};

/// A reader that returns at most `n` bytes per call.
struct Trickle<'a> {
    data: &'a [u8],
    n: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let take = self.n.min(buf.len()).min(self.data.len());
        buf[..take].copy_from_slice(&self.data[..take]);
        self.data = &self.data[take..];
        Ok(take)
    }
}

type Outcome = (Vec<Event>, Option<(ErrorKind, usize)>);

fn drain(mut next: impl FnMut() -> oxml::Result<Option<Event>>) -> Outcome {
    let mut events = Vec::new();
    loop {
        match next() {
            Ok(Some(event)) => events.push(event),
            Ok(None) => return (events, None),
            Err(e) => return (events, Some((e.kind, e.offset))),
        }
    }
}

fn in_memory(bytes: &[u8]) -> Outcome {
    match oxml::encoding::decode(bytes) {
        Ok(text) => read(&text),
        Err(e) => (Vec::new(), Some((e.kind, e.offset))),
    }
}

/// `Reader::new` over text that is already decoded.
fn read(text: &str) -> Outcome {
    match Reader::new(text) {
        Ok(mut r) => drain(|| r.next_event()),
        Err(e) => (Vec::new(), Some((e.kind, e.offset))),
    }
}

fn streamed(bytes: &[u8], n: usize) -> Outcome {
    let source = BufReader::with_capacity(1, Trickle { data: bytes, n });
    match Reader::from_reader(source) {
        Ok(mut r) => drain(|| r.next_event()),
        Err(e) => (Vec::new(), Some((e.kind, e.offset))),
    }
}

/// The length of a byte-order mark, which decoding offsets exclude.
fn bom_len(bytes: &[u8]) -> usize {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else if bytes.starts_with(&[0xFE, 0xFF])
        || bytes.starts_with(&[0xFF, 0xFE])
    {
        2
    } else {
        0
    }
}

/// What `Reader::new` reports over the text that the bytes before
/// `refused` decode to, in that text's offsets.
///
/// Those bytes can hold an earlier decoding error that `decode`
/// reported second (UTF-16 checks the length before the surrogates),
/// so the prefix shrinks until it decodes.
fn before_refusal(
    bytes: &[u8],
    mut refused: usize,
) -> Option<(ErrorKind, usize)> {
    let bom = bom_len(bytes);
    loop {
        match oxml::encoding::decode(&bytes[..bom + refused]) {
            Ok(text) => return read(&text).1,
            Err(e) if e.offset < refused => refused = e.offset,
            Err(e) => return Some((e.kind, e.offset)),
        }
    }
}

/// Whether `got` is a legitimate report for a document whose decoding
/// refused at byte `refused`: that refusal itself or an earlier one,
/// or an error found in the text before it, at or before where
/// `Reader::new` finds one there.
fn is_earlier(bytes: &[u8], refused: usize, got: &(ErrorKind, usize)) -> bool {
    match got {
        (ErrorKind::MalformedEncoding, at) => *at <= refused,
        (_, at) => {
            before_refusal(bytes, refused).is_some_and(|(kind, there)| {
                (&kind, there) == (&got.0, *at) || *at < there
            })
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&first, bytes)) = data.split_first() else {
        return;
    };
    let chunk = usize::from(first).max(1);
    let expected = in_memory(bytes);
    let got = streamed(bytes, chunk);
    match &expected {
        (events, Some((kind, offset)))
            if events.is_empty()
                && matches!(
                    kind,
                    ErrorKind::IllegalCharacter(_)
                        | ErrorKind::MalformedEncoding
                ) =>
        {
            let Some(reported) = &got.1 else {
                panic!(
                    "accepted what decoding refused ({kind:?} at {offset}): {bytes:?}"
                );
            };
            let legitimate = *reported == (kind.clone(), *offset)
                || match kind {
                    ErrorKind::MalformedEncoding => {
                        is_earlier(bytes, *offset, reported)
                    }
                    _ => {
                        reported.0 != ErrorKind::MalformedEncoding
                            && reported.1 < *offset
                    }
                };
            assert!(
                legitimate,
                "chunk {chunk}: expected {kind:?} at {offset}, got {reported:?}: {bytes:?}"
            );
        }
        _ => assert_eq!(got, expected, "chunk {chunk}: {bytes:?}"),
    }
});
