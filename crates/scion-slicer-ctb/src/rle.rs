//! CTB layer image encoding.
//!
//! Each chunk starts with a byte whose low 7 bits are the pixel value's top 7 bits. If the
//! top bit is set, a run length follows in 1 to 4 bytes, its prefix bits saying how many
//! (0xxxxxxx: 7 bits; 10xxxxxx: 14; 110xxxxx: 21; 1110xxxx: 28); otherwise the run is one
//! pixel. A 7-bit value v decodes to (v << 1) | 1, so 0x7F is 0xFF and 0 stays 0.

/// Encode a layer image, one byte per pixel, row by row.
pub fn encode(pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    let mut i = 0;
    while i < pixels.len() {
        let code = pixels[i] >> 1;
        let run = pixels[i..]
            .iter()
            .position(|&p| p >> 1 != code)
            .unwrap_or(pixels.len() - i);
        let mut left = run;
        while left > 0 {
            let len = left.min(0x0FFF_FFFF);
            push_run(&mut out, code, len as u32);
            left -= len;
        }
        i += run;
    }
    out
}

/// One run of `len` pixels whose 7-bit value is `code`.
pub fn push_run(out: &mut Vec<u8>, code: u8, len: u32) {
    if len == 0 {
        return;
    }
    if len == 1 {
        out.push(code);
        return;
    }
    out.push(code | 0x80);
    match len {
        0..=0x7F => out.push(len as u8),
        0x80..=0x3FFF => out.extend_from_slice(&[(len >> 8) as u8 | 0x80, len as u8]),
        0x4000..=0x1F_FFFF => {
            out.extend_from_slice(&[(len >> 16) as u8 | 0xC0, (len >> 8) as u8, len as u8])
        }
        _ => out.extend_from_slice(&[
            (len >> 24) as u8 | 0xE0,
            (len >> 16) as u8,
            (len >> 8) as u8,
            len as u8,
        ]),
    }
}

/// Decode a layer image of `pixel_count` pixels.
pub fn decode(data: &[u8], pixel_count: usize) -> Result<Vec<u8>, String> {
    let mut pixels = Vec::with_capacity(pixel_count);
    let mut i = 0;
    let byte = |i: usize| data.get(i).copied().ok_or("layer data ends inside a run");
    while i < data.len() {
        let head = data[i];
        i += 1;
        let code = head & 0x7F;
        let len = if head & 0x80 == 0 {
            1
        } else {
            let b0 = byte(i)? as usize;
            i += 1;
            let (extra, top) = match b0 {
                0x00..=0x7F => (0, b0),
                0x80..=0xBF => (1, b0 & 0x3F),
                0xC0..=0xDF => (2, b0 & 0x1F),
                0xE0..=0xEF => (3, b0 & 0x0F),
                _ => return Err(format!("bad run length byte {b0:#04x}")),
            };
            let mut len = top;
            for _ in 0..extra {
                len = (len << 8) | byte(i)? as usize;
                i += 1;
            }
            len
        };
        let value = if code == 0 { 0 } else { (code << 1) | 1 };
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
    fn round_trips() {
        let mut px = vec![0u8; 300_000];
        px[5..200].fill(0xFF);
        px[1000..1003].fill(0x80);
        px[250_000..].fill(0xFF);
        let decoded = decode(&encode(&px), px.len()).unwrap();
        // 7 bits survive: 0x80 comes back as 0x81.
        let expect: Vec<u8> = px
            .iter()
            .map(|&p| if p == 0 { 0 } else { ((p >> 1) << 1) | 1 })
            .collect();
        assert_eq!(decoded, expect);
    }

    #[test]
    fn length_fields_grow_with_the_run() {
        for (len, bytes) in [
            (1u32, 1usize),
            (2, 2),
            (0x7F, 2),
            (0x80, 3),
            (0x3FFF, 3),
            (0x4000, 4),
            (0x20_0000, 5),
        ] {
            let mut out = Vec::new();
            push_run(&mut out, 0x7F, len);
            assert_eq!(out.len(), bytes, "run of {len}");
        }
    }
}
