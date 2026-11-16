//! Compare the layer images of two `.goo` files for the same part, wherever each slicer
//! put it on the screen and whichever way round.
//!
//!   cargo run --release -p scion-slicer-goo --example compare -- a.goo b.goo
//!
//! For a few layers, each image is cropped to its lit pixels and the crops are compared
//! as they are, mirrored left-right, mirrored top-bottom, and turned half way round. The
//! orientation with the fewest differing pixels is the one the two slicers agree on.

use scion_slicer_goo::rle::decode_layer;
use scion_slicer_goo::GooFile;

struct Crop {
    w: usize,
    h: usize,
    px: Vec<bool>,
    at: (usize, usize),
}

fn crop(pixels: &[u8], width: usize) -> Option<Crop> {
    let height = pixels.len() / width;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..height {
        let row = &pixels[y * width..(y + 1) * width];
        if let (Some(a), Some(b)) = (row.iter().position(|&p| p != 0), row.iter().rposition(|&p| p != 0)) {
            (x0, x1, y0, y1) = (x0.min(a), x1.max(b), y0.min(y), y);
        }
    }
    if x0 == usize::MAX {
        return None;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut px = Vec::with_capacity(w * h);
    for y in y0..=y1 {
        px.extend(pixels[y * width + x0..y * width + x1 + 1].iter().map(|&p| p != 0));
    }
    Some(Crop { w, h, px, at: (x0, y0) })
}

fn differ(a: &Crop, b: &Crop, flip_x: bool, flip_y: bool) -> Option<usize> {
    if a.w != b.w || a.h != b.h {
        return None;
    }
    let mut n = 0;
    for y in 0..a.h {
        for x in 0..a.w {
            let (bx, by) = (if flip_x { a.w - 1 - x } else { x }, if flip_y { a.h - 1 - y } else { y });
            n += (a.px[y * a.w + x] != b.px[by * b.w + bx]) as usize;
        }
    }
    Some(n)
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [a, b] = &args[..] else {
        return Err("usage: compare a.goo b.goo".into());
    };
    let (ab, bb) = (std::fs::read(a).map_err(|e| e.to_string())?, std::fs::read(b).map_err(|e| e.to_string())?);
    let (fa, fb) = (GooFile::parse(&ab)?, GooFile::parse(&bb)?);
    let (w, n) = (fa.header.resolution_x as usize, fa.header.resolution_x as usize * fa.header.resolution_y as usize);
    println!("layers: {} vs {}", fa.layers.len(), fb.layers.len());
    let count = fa.layers.len().min(fb.layers.len());
    let mut picks: Vec<usize> = vec![0, 4, 5, 14, 15, count / 2, count - 1];
    picks.retain(|&i| i < count);
    picks.dedup();
    for i in picks {
        let ca = crop(&decode_layer(fa.layers[i].data, n)?, w);
        let cb = crop(&decode_layer(fb.layers[i].data, n)?, w);
        let (Some(ca), Some(cb)) = (ca, cb) else {
            println!("layer {i}: one of them is empty");
            continue;
        };
        let show = |d: Option<usize>| d.map_or("size differs".to_string(), |d| d.to_string());
        println!(
            "layer {i}: {}x{} at {:?} vs {}x{} at {:?}; differing pixels: as is {}, mirrored left-right {}, top-bottom {}, turned round {}",
            ca.w,
            ca.h,
            ca.at,
            cb.w,
            cb.h,
            cb.at,
            show(differ(&ca, &cb, false, false)),
            show(differ(&ca, &cb, true, false)),
            show(differ(&ca, &cb, false, true)),
            show(differ(&ca, &cb, true, true)),
        );
    }
    Ok(())
}
