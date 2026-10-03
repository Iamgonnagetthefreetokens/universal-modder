//! A tiny, dependency-free PNG encoder and a small 2D image buffer.
//!
//! Nothing here is clever: pixels are RGB8, the zlib stream uses uncompressed
//! deflate blocks, the CRC and Adler tables are computed in this file. That makes
//! the output byte-for-byte reproducible, which matters because the crate's
//! determinism oracle compares rendered frames as well as world hashes.

use std::io::Write;

/// A top-left-origin RGB8 image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbImage {
    pub fn new(width: u32, height: u32) -> RgbImage {
        RgbImage {
            width,
            height,
            pixels: vec![0u8; (width as usize * height as usize) * 3],
        }
    }

    /// An image filled with one colour.
    pub fn filled(width: u32, height: u32, color: (u8, u8, u8)) -> RgbImage {
        let mut img = RgbImage::new(width, height);
        img.fill(color);
        img
    }

    pub fn fill(&mut self, color: (u8, u8, u8)) {
        for p in self.pixels.chunks_exact_mut(3) {
            p[0] = color.0;
            p[1] = color.1;
            p[2] = color.2;
        }
    }

    pub fn set(&mut self, x: u32, y: u32, color: (u8, u8, u8)) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 3;
        self.pixels[i] = color.0;
        self.pixels[i + 1] = color.1;
        self.pixels[i + 2] = color.2;
    }

    pub fn get(&self, x: u32, y: u32) -> (u8, u8, u8) {
        if x >= self.width || y >= self.height {
            return (0, 0, 0);
        }
        let i = (y as usize * self.width as usize + x as usize) * 3;
        (self.pixels[i], self.pixels[i + 1], self.pixels[i + 2])
    }

    /// Axis-aligned filled rectangle, clipped to the image.
    pub fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: (u8, u8, u8)) {
        for yy in y..(y + h).min(self.height) {
            for xx in x..(x + w).min(self.width) {
                self.set(xx, yy, color);
            }
        }
    }

    /// A rectangle that keeps a per-pixel checkerboard, for a cheap texture.
    pub fn checker_rect(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        a: (u8, u8, u8),
        b: (u8, u8, u8),
    ) {
        for yy in y..(y + h).min(self.height) {
            for xx in x..(x + w).min(self.width) {
                let color = if (xx + yy) % 2 == 0 { a } else { b };
                self.set(xx, yy, color);
            }
        }
    }
}

/// The 8-byte PNG signature.
const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Encode an image as a PNG file (RGB8, filter type 0 on every row).
pub fn encode_png(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024 + img.pixels.len());
    out.extend_from_slice(&PNG_SIG);
    // IHDR
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // colour type: truecolour
    ihdr.push(0); // deflate
    ihdr.push(0); // filter method 0
    ihdr.push(0); // no interlace
    write_chunk(&mut out, b"IHDR", &ihdr);
    // IDAT: one filter byte per row, then the raw RGB.
    let stride = img.width as usize * 3;
    let mut raw = Vec::with_capacity(stride * img.height as usize + img.height as usize);
    for row in img.pixels.chunks_exact(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    write_chunk(&mut out, b"IDAT", &zlib_store(&raw));
    write_chunk(&mut out, b"IEND", &[]);
    out
}

/// Write an image to disk as PNG.
pub fn write_png(path: &str, img: &RgbImage) -> std::io::Result<()> {
    let bytes = encode_png(img);
    let mut f = std::fs::File::create(path)?;
    f.write_all(&bytes)?;
    f.flush()
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// A zlib stream whose deflate payload is a run of uncompressed blocks.
pub fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 8);
    out.push(0x78); // CM=8 (deflate), CINFO=7 (32K window)
    out.push(0x01); // no preset dictionary, fastest compression
    let mut rest = data;
    if rest.is_empty() {
        // An empty final stored block.
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(!0u16).to_le_bytes());
    }
    while !rest.is_empty() {
        let take = rest.len().min(65535);
        let last = take == rest.len();
        out.push(if last { 1 } else { 0 });
        let len = take as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&rest[..take]);
        rest = &rest[take..];
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = crc_table();

/// CRC-32 as used by PNG chunks.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// Adler-32 as used by zlib.
pub fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Parse the width and height back out of a PNG produced here (used by tests).
pub fn probe_png(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || bytes[..8] != PNG_SIG {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_match_known_vectors() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn stored_zlib_roundtrips_through_a_reader() {
        // 70_000 bytes forces three stored blocks (65535 + 4465).
        let data: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        let stream = zlib_store(&data);
        assert_eq!(stream[0], 0x78);
        assert_eq!(&stream[1..2], &[0x01]);
        let inflated = inflate_stored(&stream);
        assert_eq!(inflated, data);
    }

    /// Minimal reader for the uncompressed-block subset this file emits.
    fn inflate_stored(stream: &[u8]) -> Vec<u8> {
        let mut i = 2;
        let mut out = Vec::new();
        loop {
            let header = stream[i];
            i += 1;
            assert_eq!(header & 0b110, 0, "only stored blocks are written");
            let len = u16::from_le_bytes(stream[i..i + 2].try_into().unwrap()) as usize;
            let nlen = u16::from_le_bytes(stream[i + 2..i + 4].try_into().unwrap());
            assert_eq!(nlen, !(len as u16));
            i += 4;
            out.extend_from_slice(&stream[i..i + len]);
            i += len;
            if header & 1 == 1 {
                break;
            }
        }
        let adler = u32::from_be_bytes(stream[i..i + 4].try_into().unwrap());
        assert_eq!(adler, adler32(&out));
        out
    }

    #[test]
    fn png_has_a_valid_header_and_roundtrips_pixels() {
        let mut img = RgbImage::filled(4, 3, (10, 20, 30));
        img.set(2, 1, (200, 100, 50));
        img.rect(3, 2, 5, 5, (1, 2, 3)); // clipped to 4x3
        let bytes = encode_png(&img);
        assert_eq!(probe_png(&bytes), Some((4, 3)));
        assert_eq!(&bytes[12..16], b"IHDR");
        assert!(bytes.ends_with(&[0xae, 0x42, 0x60, 0x82]), "IEND crc");
        assert_eq!(img.get(2, 1), (200, 100, 50));
        assert_eq!(img.get(3, 2), (1, 2, 3));
        assert_eq!(img.get(9, 9), (0, 0, 0), "out of bounds reads are black");
    }

    #[test]
    fn encoding_is_deterministic() {
        let mut a = RgbImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                a.set(x, y, ((x * 30) as u8, (y * 30) as u8, 7));
            }
        }
        let b = a.clone();
        assert_eq!(encode_png(&a), encode_png(&b));
    }
}
