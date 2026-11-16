//! The run-length encoding of one layer image.
//!
//! A layer's data starts with the byte 0x55 and ends with a checksum: the one's complement
//! of the sum of every byte in between. In between are chunks. The first byte of a chunk
//! says what it holds:
//!
//! - bits 7–6: `00` black (0x00) pixels, `11` white (0xFF) pixels, `01` a gray value, which
//!   follows in the next byte, `10` a difference from the previous pixel's value;
//! - bits 5–4: how many more bytes the run length takes (0–3), most significant first;
//! - bits 3–0: the low four bits of the run length.
//!
//! SatelLite writes one chunk per run of equal pixels, with the shortest length field that
//! fits, and never uses difference chunks for black-and-white layers. [`encode_layer`]
//! does exactly that, so it reproduces SatelLite's bytes for the same pixels.

/// The byte every layer's data starts with.
pub const LAYER_MAGIC: u8 = 0x55;

/// The longest run one chunk can hold: 4 + 24 bits.
pub const MAX_RUN: u32 = (1 << 28) - 1;

/// Encode a layer image (one byte per pixel, row by row) into `.goo` layer data,
/// including the leading 0x55 and the trailing checksum.
pub fn encode_layer(pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    out.push(LAYER_MAGIC);
    let mut i = 0;
    while i < pixels.len() {
        let value = pixels[i];
        let run = pixels[i..]
            .iter()
            .position(|&p| p != value)
            .unwrap_or(pixels.len() - i);
        let mut left = run;
        while left > 0 {
            let len = left.min(MAX_RUN as usize);
            push_run(&mut out, value, len as u32);
            left -= len;
        }
        i += run;
    }
    out.push(checksum(&out[1..]));
    out
}

/// The checksum SatelLite writes: the one's complement of the byte sum.
pub fn checksum(chunks: &[u8]) -> u8 {
    !chunks.iter().fold(0u8, |sum, &b| sum.wrapping_add(b))
}

fn push_run(out: &mut Vec<u8>, value: u8, len: u32) {
    debug_assert!(len > 0 && len <= MAX_RUN);
    let kind: u8 = match value {
        0x00 => 0b00,
        0xFF => 0b11,
        _ => 0b01,
    };
    let extra: u8 = match len {
        0..=0xF => 0,
        0x10..=0xFFF => 1,
        0x1000..=0xF_FFFF => 2,
        _ => 3,
    };
    out.push((kind << 6) | (extra << 4) | (len & 0xF) as u8);
    if kind == 0b01 {
        out.push(value);
    }
    for k in (0..extra).rev() {
        out.push((len >> (4 + 8 * k as u32)) as u8);
    }
}

/// Decode `.goo` layer data (with its 0x55 and checksum) into `pixel_count` pixels.
///
/// Checks the lead byte, the checksum and the pixel count, and reads every chunk type,
/// difference chunks included.
pub fn decode_layer(data: &[u8], pixel_count: usize) -> Result<Vec<u8>, String> {
    let (&first, rest) = data.split_first().ok_or("empty layer data")?;
    if first != LAYER_MAGIC {
        return Err(format!("layer data starts with {first:#04x}, not 0x55"));
    }
    let (&sum, chunks) = rest.split_last().ok_or("layer data has no checksum")?;
    if checksum(chunks) != sum {
        return Err("layer checksum does not match".into());
    }
    let mut pixels = Vec::with_capacity(pixel_count);
    let mut value: u8 = 0;
    let mut i = 0;
    let byte = |i: usize| {
        chunks
            .get(i)
            .copied()
            .ok_or("layer data ends inside a chunk")
    };
    while i < chunks.len() {
        let head = chunks[i];
        let extra = (head >> 4) & 0b11;
        i += 1;
        let len: usize = if head >> 6 == 0b10 {
            // A difference from the previous pixel: bit 5 is the sign, bit 4 says whether
            // a length byte follows (otherwise the run is one pixel).
            let diff = head & 0xF;
            value = if extra & 0b10 != 0 {
                value.wrapping_sub(diff)
            } else {
                value.wrapping_add(diff)
            };
            if extra & 0b01 != 0 {
                i += 1;
                byte(i - 1)? as usize
            } else {
                1
            }
        } else {
            value = match head >> 6 {
                0b00 => 0x00,
                0b11 => 0xFF,
                _ => {
                    i += 1;
                    byte(i - 1)?
                }
            };
            let mut len = (head & 0xF) as usize;
            for k in (0..extra).rev() {
                len |= (byte(i)? as usize) << (4 + 8 * k as usize);
                i += 1;
            }
            len
        };
        if pixels.len() + len > pixel_count {
            return Err(format!("layer data holds more than {pixel_count} pixels"));
        }
        pixels.resize(pixels.len() + len, value);
    }
    if pixels.len() != pixel_count {
        return Err(format!(
            "layer data holds {} pixels, expected {pixel_count}",
            pixels.len()
        ));
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_examples_encode_as_published() {
        // From Elegoo's spec: black run of 0x555657f, gray 0xaa run of 0xbbcc155,
        // black run of 5, white run of 0xcbbaaff1 does not fit 28 bits and is skipped.
        let mut out = Vec::new();
        push_run(&mut out, 0x00, 0x555657f);
        assert_eq!(out, [0b0011_1111, 0x55, 0x56, 0x57]);
        out.clear();
        push_run(&mut out, 0xaa, 0xbbcc155);
        assert_eq!(out, [0b0111_0101, 0xaa, 0xbb, 0xcc, 0x15]);
        out.clear();
        push_run(&mut out, 0x00, 5);
        assert_eq!(out, [0b0000_0101]);
    }

    #[test]
    fn runs_use_the_shortest_length_field() {
        for (len, bytes) in [
            (15u32, 1usize),
            (16, 2),
            (0xFFF, 2),
            (0x1000, 3),
            (0xF_FFFF, 3),
            (0x10_0000, 4),
        ] {
            let mut out = Vec::new();
            push_run(&mut out, 0xFF, len);
            assert_eq!(out.len(), bytes, "run of {len}");
        }
    }

    #[test]
    fn round_trips_black_white_and_gray() {
        let mut pixels = vec![0u8; 100_000];
        pixels[10..5000].fill(0xFF);
        pixels[7000..7003].fill(0x80);
        pixels[90_000..].fill(0xFF);
        let data = encode_layer(&pixels);
        assert_eq!(data[0], LAYER_MAGIC);
        assert_eq!(decode_layer(&data, pixels.len()).unwrap(), pixels);
    }

    #[test]
    fn decodes_difference_chunks() {
        // gray 0x10 x1, +2 x1, +1 x3 (with a length byte), -3 x1
        let chunks = [0b0100_0001, 0x10, 0b1000_0010, 0b1001_0001, 3, 0b1010_0011];
        let mut data = vec![LAYER_MAGIC];
        data.extend_from_slice(&chunks);
        data.push(checksum(&chunks));
        assert_eq!(
            decode_layer(&data, 6).unwrap(),
            [0x10, 0x12, 0x13, 0x13, 0x13, 0x10]
        );
    }

    #[test]
    fn rejects_bad_checksums_and_counts() {
        let mut data = encode_layer(&[0, 0, 0xFF]);
        assert!(decode_layer(&data, 4).is_err());
        let last = data.len() - 1;
        data[last] ^= 1;
        assert!(decode_layer(&data, 3).is_err());
    }
}
