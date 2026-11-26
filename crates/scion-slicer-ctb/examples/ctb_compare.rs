//! Compare two encrypted CTB files field by field and layer by layer.
//!
//!   cargo run --release -p scion-slicer-ctb --example ctb_compare -- ours.ctb theirs.ctb [--pixels]
//!
//! Prints every header and settings field that differs, every layer-record field that
//! differs (data offsets and lengths are left out: they follow from the images), and with
//! --pixels each layer's lit-pixel counts and intersection over union, after lining the two
//! images up on their lit pixels' bounding boxes.

use scion_slicer_ctb::fields::Record;
use scion_slicer_ctb::CtbFile;

fn diff(title: &str, a: &Record, b: &Record, skip: &[&str]) -> usize {
    let mut n = 0;
    for ((name, va), (_, vb)) in a.fields.iter().zip(&b.fields) {
        if va != vb && !skip.contains(name) {
            println!("  {title} {name}: {va} vs {vb}");
            n += 1;
        }
    }
    n
}

fn bbox(px: &[u8], w: usize) -> Option<(usize, usize, usize, usize)> {
    let h = px.len() / w;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..h {
        let row = &px[y * w..(y + 1) * w];
        if let (Some(a), Some(b)) = (
            row.iter().position(|&p| p != 0),
            row.iter().rposition(|&p| p != 0),
        ) {
            (x0, x1, y0, y1) = (x0.min(a), x1.max(b), y0.min(y), y);
        }
    }
    (x0 != usize::MAX).then_some((x0, y0, x1, y1))
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [a, b, rest @ ..] = &args[..] else {
        return Err("usage: ctb_compare ours.ctb theirs.ctb [--pixels]".into());
    };
    let pixels = rest.iter().any(|r| r == "--pixels");
    let (ba, bb) = (
        std::fs::read(a).map_err(|e| e.to_string())?,
        std::fs::read(b).map_err(|e| e.to_string())?,
    );
    let (fa, fb) = (CtbFile::parse(&ba)?, CtbFile::parse(&bb)?);
    println!(
        "{} bytes vs {} bytes; signature {} vs {}",
        fa.len, fb.len, fa.signature_ok, fb.signature_ok
    );
    let crc_ok = |b: &[u8]| crc32fast::hash(&b[..b.len() - 4]).to_le_bytes() == b[b.len() - 4..];
    println!("trailing CRC-32 {} vs {}", crc_ok(&ba), crc_ok(&bb));
    println!(
        "machine name {:?} vs {:?}",
        fa.machine_name, fb.machine_name
    );
    println!("header:");
    diff("header", &fa.header, &fb.header, &[]);
    println!("settings:");
    diff("settings", &fa.settings, &fb.settings, &[]);
    let skip = [
        "LayerDataOffset",
        "PageNumber",
        "LayerDataLength",
        "EncryptedDataOffset",
        "EncryptedDataLength",
    ];
    println!("layers: {} vs {}", fa.layers.len(), fb.layers.len());
    let mut record_diffs = 0;
    for (i, (la, lb)) in fa.layers.iter().zip(&fb.layers).enumerate() {
        record_diffs += diff(&format!("L{i}"), &la.def, &lb.def, &skip);
    }
    println!("{record_diffs} layer-record fields differ");
    if pixels {
        let (w, n) = (fa.settings.u("ResolutionX") as usize, fa.pixel_count());
        let h = n / w;
        // One shift for every layer, from the layer with the most lit pixels: the two
        // slicers may place the part differently, but it doesn't move between layers.
        let decoded: Vec<(Vec<u8>, Vec<u8>)> = fa
            .layers
            .iter()
            .zip(&fb.layers)
            .map(|(la, lb)| Ok((la.pixels(n)?, lb.pixels(n)?)))
            .collect::<Result<_, String>>()?;
        let lit = |p: &[u8]| p.iter().filter(|&&v| v != 0).count();
        let reference = (0..decoded.len())
            .max_by_key(|&i| lit(&decoded[i].1))
            .unwrap_or(0);
        let (dx, dy) = match (
            bbox(&decoded[reference].0, w),
            bbox(&decoded[reference].1, w),
        ) {
            (Some(a), Some(b)) => (b.0 as i64 - a.0 as i64, b.1 as i64 - a.1 as i64),
            _ => (0, 0),
        };
        println!("aligned by layer {reference}'s bounding box: shift ({dx}, {dy})");
        let mut worst = (1.0f64, 0usize);
        for (i, (pa, pb)) in decoded.iter().enumerate() {
            let (mut inter, mut union, mut only_a, mut only_b) = (0u64, 0u64, 0u64, 0u64);
            for y in 0..h {
                for x in 0..w {
                    let a_on = pa[y * w + x] != 0;
                    let (xb, yb) = (x as i64 + dx, y as i64 + dy);
                    let b_on = xb >= 0
                        && yb >= 0
                        && (xb as usize) < w
                        && (yb as usize) < h
                        && pb[yb as usize * w + xb as usize] != 0;
                    inter += (a_on && b_on) as u64;
                    union += (a_on || b_on) as u64;
                    only_a += (a_on && !b_on) as u64;
                    only_b += (b_on && !a_on) as u64;
                }
            }
            let iou = if union == 0 {
                1.0
            } else {
                inter as f64 / union as f64
            };
            if iou < worst.0 {
                worst = (iou, i);
            }
            println!(
                "  L{i}: z {} vs {}; lit {} vs {}; IoU {iou:.6}; only ours {only_a}, only theirs {only_b}",
                fa.layers[i].def.f("PositionZ"),
                fb.layers[i].def.f("PositionZ"),
                lit(pa),
                lit(pb)
            );
        }
        println!("lowest IoU {:.6} on layer {}", worst.0, worst.1);
    }
    Ok(())
}
