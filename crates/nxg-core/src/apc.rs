//! Byte-level pre-filter that pulls APC strings out of the child output.
//!
//! vte drops APC strings (`ESC _ … ESC \`) without a callback, but the
//! kitty graphics protocol lives in them. [`ApcFilter`] splits the stream
//! into runs for the VT parser and complete APC payloads, in input order,
//! so side effects keep their ordering. Sequences may be split across
//! [`ApcFilter::feed`] calls.

/// Largest APC payload kept; longer ones are dropped whole. Kitty graphics
/// chunks are at most 4096 bytes, so this only bounds hostile input.
pub const MAX_APC: usize = 4 * 1024 * 1024;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// A piece of the input, in stream order.
#[derive(Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// Bytes for the VT parser.
    Text(&'a [u8]),
    /// The payload of a complete APC string (without `ESC _` and the
    /// terminator).
    Apc(&'a [u8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ground,
    /// Saw `ESC` in ground at the end of the previous chunk.
    GroundEsc,
    Apc,
    /// Saw `ESC` inside an APC.
    ApcEsc,
    /// The APC grew past [`MAX_APC`]; skip to its terminator.
    Overflow,
    OverflowEsc,
}

/// Streaming APC extractor; see the module docs.
#[derive(Debug)]
pub struct ApcFilter {
    mode: Mode,
    buf: Vec<u8>,
}

impl Default for ApcFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl ApcFilter {
    pub fn new() -> Self {
        Self {
            mode: Mode::Ground,
            buf: Vec::new(),
        }
    }

    /// Splits `bytes` into [`Event`]s, calling `sink` for each in order.
    pub fn feed(&mut self, bytes: &[u8], mut sink: impl FnMut(Event<'_>)) {
        let mut i = 0;
        // Start of the pending ground run inside `bytes`.
        let mut run = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match self.mode {
                Mode::Ground => {
                    if b == ESC {
                        if let Some(&next) = bytes.get(i + 1) {
                            if next == b'_' {
                                if run < i {
                                    sink(Event::Text(&bytes[run..i]));
                                }
                                self.mode = Mode::Apc;
                                self.buf.clear();
                                i += 2;
                                run = i;
                                continue;
                            }
                        } else {
                            // Lone trailing ESC: decide on the next chunk.
                            if run < i {
                                sink(Event::Text(&bytes[run..i]));
                            }
                            self.mode = Mode::GroundEsc;
                            i += 1;
                            run = i;
                            continue;
                        }
                    }
                    i += 1;
                }
                Mode::GroundEsc => {
                    if b == b'_' {
                        self.mode = Mode::Apc;
                        self.buf.clear();
                        i += 1;
                    } else {
                        sink(Event::Text(b"\x1b"));
                        self.mode = Mode::Ground;
                    }
                    run = i;
                }
                Mode::Apc | Mode::Overflow => {
                    let overflow = self.mode == Mode::Overflow;
                    // Copy everything up to the next special byte at once.
                    let end = bytes[i..]
                        .iter()
                        .position(|&c| matches!(c, ESC | BEL | CAN | SUB))
                        .map_or(bytes.len(), |p| i + p);
                    if !overflow {
                        if self.buf.len() + (end - i) > MAX_APC {
                            self.mode = Mode::Overflow;
                            self.buf = Vec::new();
                        } else {
                            self.buf.extend_from_slice(&bytes[i..end]);
                        }
                    }
                    i = end;
                    let Some(&c) = bytes.get(i) else { break };
                    i += 1;
                    match c {
                        ESC => {
                            self.mode = if self.mode == Mode::Overflow {
                                Mode::OverflowEsc
                            } else {
                                Mode::ApcEsc
                            };
                        }
                        BEL => self.finish(&mut sink),
                        _ => self.abort(), // CAN / SUB cancel the string.
                    }
                    run = i;
                }
                Mode::ApcEsc | Mode::OverflowEsc => {
                    if b == b'\\' {
                        i += 1;
                        self.finish(&mut sink);
                    } else {
                        // Any other ESC ends the string unterminated; the
                        // ESC starts a new sequence in ground.
                        self.abort();
                        self.mode = Mode::GroundEsc;
                    }
                    run = i;
                }
            }
        }
        if matches!(self.mode, Mode::Ground) && run < bytes.len() {
            sink(Event::Text(&bytes[run..]));
        }
    }

    fn finish(&mut self, sink: &mut impl FnMut(Event<'_>)) {
        if self.mode != Mode::Overflow && self.mode != Mode::OverflowEsc {
            sink(Event::Apc(&self.buf));
        }
        self.abort();
    }

    fn abort(&mut self) {
        self.mode = Mode::Ground;
        self.buf.clear();
        if self.buf.capacity() > 64 * 1024 {
            self.buf = Vec::new();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Out {
        Text(Vec<u8>),
        Apc(Vec<u8>),
    }

    /// Feeds `chunks` and returns the events, merging adjacent text runs.
    fn run(chunks: &[&[u8]]) -> Vec<Out> {
        let mut filter = ApcFilter::new();
        let mut out: Vec<Out> = Vec::new();
        for chunk in chunks {
            filter.feed(chunk, |event| match event {
                Event::Text(t) => match out.last_mut() {
                    Some(Out::Text(prev)) => prev.extend_from_slice(t),
                    _ => out.push(Out::Text(t.to_vec())),
                },
                Event::Apc(a) => out.push(Out::Apc(a.to_vec())),
            });
        }
        out
    }

    fn text(t: &[u8]) -> Out {
        Out::Text(t.to_vec())
    }

    fn apc(a: &[u8]) -> Out {
        Out::Apc(a.to_vec())
    }

    #[test]
    fn passes_plain_text_through() {
        assert_eq!(run(&[b"hello \x1b[1mworld"]), [text(b"hello \x1b[1mworld")]);
    }

    #[test]
    fn extracts_apc_terminated_by_st_between_text() {
        assert_eq!(
            run(&[b"a\x1b_Gi=1;AAAA\x1b\\b"]),
            [text(b"a"), apc(b"Gi=1;AAAA"), text(b"b")]
        );
    }

    #[test]
    fn accepts_bel_terminator() {
        assert_eq!(run(&[b"\x1b_Gx\x07z"]), [apc(b"Gx"), text(b"z")]);
    }

    #[test]
    fn handles_sequences_split_at_every_byte() {
        let input = b"ab\x1b_Ga=T;QUJD\x1b\\cd\x1b[0m\x1b_Gq\x07e";
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        assert_eq!(
            run(&chunks),
            [
                text(b"ab"),
                apc(b"Ga=T;QUJD"),
                text(b"cd\x1b[0m"),
                apc(b"Gq"),
                text(b"e")
            ]
        );
    }

    #[test]
    fn trailing_esc_is_released_when_not_an_apc() {
        assert_eq!(run(&[b"x\x1b", b"[1m"]), [text(b"x\x1b[1m")]);
    }

    #[test]
    fn other_escapes_inside_apc_abort_it() {
        assert_eq!(run(&[b"\x1b_Gabc\x1b[1mz"]), [text(b"\x1b[1mz")]);
    }

    #[test]
    fn can_cancels_apc() {
        assert_eq!(run(&[b"\x1b_Gabc\x18ok"]), [text(b"ok")]);
    }

    #[test]
    fn oversized_apc_is_dropped_and_stream_recovers() {
        let mut big = b"\x1b_G".to_vec();
        big.resize(MAX_APC + 100, b'A');
        let chunks: Vec<&[u8]> = big.chunks(4096).collect();
        let mut all = chunks.clone();
        all.push(b"\x1b\\after\x1b_Gok\x1b\\");
        assert_eq!(run(&all), [text(b"after"), apc(b"Gok")]);
    }

    #[test]
    fn apc_at_exact_cap_is_kept() {
        let mut input = b"\x1b_".to_vec();
        input.resize(2 + MAX_APC, b'A');
        input.extend_from_slice(b"\x1b\\");
        let out = run(&[&input]);
        assert!(matches!(&out[..], [Out::Apc(a)] if a.len() == MAX_APC));
    }
}
