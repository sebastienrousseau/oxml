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

use std::io::{BufReader, Read};

use libfuzzer_sys::fuzz_target;
use oxml::stream::{Event, Reader};
use oxml::ErrorKind;

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
    let text = match oxml::encoding::decode(bytes) {
        Ok(text) => text,
        Err(e) => return (Vec::new(), Some((e.kind, e.offset))),
    };
    match Reader::new(&text) {
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
                    ErrorKind::IllegalCharacter(_) | ErrorKind::MalformedEncoding
                ) =>
        {
            let Some((got_kind, got_offset)) = &got.1 else {
                panic!("accepted what decoding refused ({kind:?} at {offset}): {bytes:?}");
            };
            assert!(
                (got_kind, got_offset) == (kind, offset) || got_offset < offset,
                "chunk {chunk}: expected {kind:?} at {offset}, got {got_kind:?} at {got_offset}: {bytes:?}"
            );
        }
        _ => assert_eq!(got, expected, "chunk {chunk}: {bytes:?}"),
    }
});
