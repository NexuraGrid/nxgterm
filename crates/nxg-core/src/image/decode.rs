//! Payload decoding: base64, zlib, PNG and raw RGB/RGBA to RGBA8.
//!
//! Every function bounds its output, so hostile input cannot make the
//! terminal allocate without limit.

use std::fmt;
use std::io::Read;

/// Largest width or height accepted for any image.
pub const MAX_DIMENSION: u32 = 10_000;
/// Largest decoded payload (raw pixels, PNG or zlib output) in bytes.
pub const MAX_DATA: usize = 128 * 1024 * 1024;

/// Why a payload could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Malformed base64.
    Base64,
    /// Malformed or truncated zlib stream.
    Zlib,
    /// Malformed PNG.
    Png(String),
    /// Fewer bytes than `width * height * channels`.
    NotEnoughData,
    /// Zero or oversized dimensions.
    BadSize,
    /// Output would exceed [`MAX_DATA`].
    TooLarge,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Base64 => f.write_str("invalid base64"),
            Self::Zlib => f.write_str("invalid zlib data"),
            Self::Png(reason) => write!(f, "invalid PNG: {reason}"),
            Self::NotEnoughData => f.write_str("insufficient image data"),
            Self::BadSize => f.write_str("invalid image dimensions"),
            Self::TooLarge => f.write_str("image data too large"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Incremental base64 decoder (standard alphabet). Input may be split
/// anywhere; padding and whitespace are accepted.
#[derive(Debug, Default, Clone)]
pub struct Base64 {
    quad: [u8; 4],
    filled: usize,
    /// Saw `=`: only more padding may follow.
    done: bool,
}

impl Base64 {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `input`, appending to `out`.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), DecodeError> {
        for &c in input {
            let value = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' | b'-' => 62,
                b'/' | b'_' => 63,
                b'=' => {
                    self.done = true;
                    continue;
                }
                b' ' | b'\n' | b'\r' | b'\t' => continue,
                _ => return Err(DecodeError::Base64),
            };
            if self.done {
                return Err(DecodeError::Base64);
            }
            self.quad[self.filled] = value;
            self.filled += 1;
            if self.filled == 4 {
                let [a, b, c, d] = self.quad;
                out.extend_from_slice(&[a << 2 | b >> 4, b << 4 | c >> 2, c << 6 | d]);
                self.filled = 0;
            }
        }
        Ok(())
    }

    /// Flushes a trailing partial group (unpadded input), appending to
    /// `out`.
    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), DecodeError> {
        let [a, b, c, _] = self.quad;
        match self.filled {
            0 => {}
            2 => out.push(a << 2 | b >> 4),
            3 => out.extend_from_slice(&[a << 2 | b >> 4, b << 4 | c >> 2]),
            _ => return Err(DecodeError::Base64),
        }
        *self = Self::default();
        Ok(())
    }
}

/// Decodes a complete base64 string.
pub fn base64(input: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut decoder = Base64::new();
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    decoder.feed(input, &mut out)?;
    decoder.finish(&mut out)?;
    Ok(out)
}

/// Inflates a zlib stream, failing past `limit` output bytes.
pub fn zlib(data: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let reader = flate2::read::ZlibDecoder::new(data);
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| DecodeError::Zlib)?;
    if out.len() > limit {
        return Err(DecodeError::TooLarge);
    }
    Ok(out)
}

/// A decoded RGBA8 picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

fn check_size(width: u32, height: u32) -> Result<usize, DecodeError> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(DecodeError::BadSize);
    }
    let len = width as usize * height as usize * 4;
    if len > MAX_DATA {
        return Err(DecodeError::TooLarge);
    }
    Ok(len)
}

/// Raw 24-bit RGB (`channels` 3) or 32-bit RGBA (`channels` 4) pixels.
/// Extra trailing bytes are ignored.
pub fn raw(data: &[u8], width: u32, height: u32, channels: usize) -> Result<Rgba, DecodeError> {
    let len = check_size(width, height)?;
    let count = len / 4;
    let needed = count * channels;
    if data.len() < needed {
        return Err(DecodeError::NotEnoughData);
    }
    let pixels = match channels {
        4 => data[..needed].to_vec(),
        3 => {
            let mut out = Vec::with_capacity(len);
            for px in data[..needed].chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        _ => return Err(DecodeError::BadSize),
    };
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// Decodes a PNG of any color type and depth to RGBA8.
pub fn png(data: &[u8]) -> Result<Rgba, DecodeError> {
    let err = |e: png::DecodingError| DecodeError::Png(e.to_string());
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits { bytes: MAX_DATA });
    let mut reader = decoder.read_info().map_err(err)?;
    let info = reader.info();
    check_size(info.width, info.height)?;
    let size = reader.output_buffer_size().ok_or(DecodeError::TooLarge)?;
    if size > MAX_DATA {
        return Err(DecodeError::TooLarge);
    }
    let mut buf = vec![0; size];
    let frame = reader.next_frame(&mut buf).map_err(err)?;
    buf.truncate(frame.buffer_size());
    let (width, height) = (frame.width, frame.height);
    let count = width as usize * height as usize;
    let pixels = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(DecodeError::Png("unexpanded palette".into())),
    };
    if pixels.len() < count * 4 {
        return Err(DecodeError::NotEnoughData);
    }
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Encodes `rgba` (`width x height`) as an RGBA8 PNG.
    pub(crate) fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(rgba).unwrap();
        writer.finish().unwrap();
        out
    }

    /// Standard padded base64.
    pub(crate) fn encode_base64(data: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub(crate) fn encode_zlib(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn base64_decodes_padded_and_unpadded() {
        assert_eq!(base64(b"aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64(b"aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64(b"aGk=").unwrap(), b"hi");
        assert_eq!(base64(b"").unwrap(), b"");
        assert_eq!(base64(b"YQ==").unwrap(), b"a");
    }

    #[test]
    fn base64_streams_across_arbitrary_splits() {
        let mut decoder = Base64::new();
        let mut out = Vec::new();
        for part in [&b"aG"[..], b"VsbG8gd2", b"9y", b"bGQ="] {
            decoder.feed(part, &mut out).unwrap();
        }
        decoder.finish(&mut out).unwrap();
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn base64_rejects_garbage() {
        assert_eq!(base64(b"a$bc"), Err(DecodeError::Base64));
        assert_eq!(base64(b"a"), Err(DecodeError::Base64));
        assert_eq!(base64(b"YQ==YQ"), Err(DecodeError::Base64));
    }

    #[test]
    fn base64_round_trips_with_test_encoder() {
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(base64(encode_base64(&data).as_bytes()).unwrap(), data);
    }

    #[test]
    fn zlib_inflates_and_enforces_limit() {
        let data = vec![7u8; 1000];
        let packed = encode_zlib(&data);
        assert_eq!(zlib(&packed, 1000).unwrap(), data);
        assert_eq!(zlib(&packed, 999), Err(DecodeError::TooLarge));
        assert_eq!(zlib(b"not zlib", 1000), Err(DecodeError::Zlib));
    }

    #[test]
    fn raw_rgb_gets_opaque_alpha() {
        let rgba = raw(&[1, 2, 3, 4, 5, 6], 2, 1, 3).unwrap();
        assert_eq!(rgba.pixels, [1, 2, 3, 255, 4, 5, 6, 255]);
        let rgba = raw(&[1, 2, 3, 4, 9], 1, 1, 4).unwrap();
        assert_eq!(rgba.pixels, [1, 2, 3, 4]);
    }

    #[test]
    fn raw_checks_size_and_data() {
        assert_eq!(raw(&[0; 5], 2, 1, 3), Err(DecodeError::NotEnoughData));
        assert_eq!(raw(&[], 0, 1, 3), Err(DecodeError::BadSize));
        assert_eq!(raw(&[], MAX_DIMENSION + 1, 1, 4), Err(DecodeError::BadSize));
        assert_eq!(
            raw(&[], MAX_DIMENSION, MAX_DIMENSION, 4),
            Err(DecodeError::TooLarge)
        );
    }

    #[test]
    fn png_decodes_to_rgba() {
        let pixels = [255, 0, 0, 255, 0, 255, 0, 128];
        let decoded = png(&encode_png(2, 1, &pixels)).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.pixels, pixels);
    }

    #[test]
    fn png_expands_rgb_and_gray() {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Grayscale);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[77]).unwrap();
        writer.finish().unwrap();
        assert_eq!(png(&out).unwrap().pixels, [77, 77, 77, 255]);
    }

    #[test]
    fn png_rejects_garbage_and_truncation() {
        assert!(matches!(png(b"nope"), Err(DecodeError::Png(_))));
        let full = encode_png(4, 4, &[9; 64]);
        assert!(png(&full[..full.len() / 2]).is_err());
    }
}
