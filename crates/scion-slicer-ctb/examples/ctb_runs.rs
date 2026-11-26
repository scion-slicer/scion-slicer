//! Print the lit runs across the middle row of a layer, and each run's height in rows:
//! the pixel size of each part in that layer.
//!
//!   cargo run --release -p scion-slicer-ctb --example ctb_runs -- file.ctb LAYER

use scion_slicer_ctb::CtbFile;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, layer] = &args[..] else {
        return Err("usage: ctb_runs file.ctb LAYER".into());
    };
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let f = CtbFile::parse(&bytes)?;
    let w = f.settings.u("ResolutionX") as usize;
    let px =
        f.layers[layer.parse::<usize>().map_err(|e| e.to_string())?].pixels(f.pixel_count())?;
    let h = px.len() / w;
    let lit_rows: Vec<usize> = (0..h)
        .filter(|&y| px[y * w..(y + 1) * w].iter().any(|&p| p != 0))
        .collect();
    let mid = lit_rows[lit_rows.len() / 2];
    let row = &px[mid * w..(mid + 1) * w];
    let mut x = 0;
    while x < w {
        if row[x] == 0 {
            x += 1;
            continue;
        }
        let start = x;
        while x < w && row[x] != 0 {
            x += 1;
        }
        let col = (start + x) / 2;
        let ys: Vec<usize> = (0..h).filter(|&y| px[y * w + col] != 0).collect();
        println!(
            "x {start}..{} ({} px wide), rows {}..{} ({} px tall) at column {col}",
            x - 1,
            x - start,
            ys[0],
            ys[ys.len() - 1],
            ys.len()
        );
    }
    Ok(())
}
