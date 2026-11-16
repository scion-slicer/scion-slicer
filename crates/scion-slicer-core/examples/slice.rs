//! Slice an STL into a `.goo` file for the Saturn 4 Ultra 16K with the SirayaTech Fast
//! (Navy Grey) settings.
//!
//!   cargo run --release -p scion-slicer-core --example slice -- part.stl part.goo [options]
//!
//!   --at X Y        put the corner of the part's bounding box X, Y mm from the screen's
//!                   corner (default: centred)
//!   --rotate DEG    turn the part about the vertical axis first
//!   --outline MM    bottom outline compensation (negative shrinks)
//!   --hole MM       bottom hole compensation (negative opens)
//!   --no-fade       give the transition layers the full compensation

use std::path::Path;

use scion_slicer_core::slice::{slice_stl_to_goo, BottomCompensation, Placement, SliceOptions};
use scion_slicer_goo::{Machine, Resin};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: slice part.stl part.goo [--at X Y] [--rotate DEG] [--outline MM] [--hole MM] [--no-fade]";
    let (Some(stl), Some(goo)) = (args.first(), args.get(1)) else {
        return Err(usage.into());
    };
    let mut options = SliceOptions::default();
    let mut comp = BottomCompensation {
        outline_mm: 0.0,
        hole_mm: 0.0,
        fade: true,
    };
    let mut rest = args[2..].iter();
    let num = |v: Option<&String>, what: &str| -> Result<f64, String> {
        v.ok_or(format!("{what} needs a value"))?
            .parse::<f64>()
            .map_err(|e| format!("{what}: {e}"))
    };
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--at" => {
                options.placement = Placement::Corner {
                    x_mm: num(rest.next(), "--at")?,
                    y_mm: num(rest.next(), "--at")?,
                }
            }
            "--rotate" => options.rotate_z_deg = num(rest.next(), "--rotate")?,
            "--outline" => comp.outline_mm = num(rest.next(), "--outline")? as f32,
            "--hole" => comp.hole_mm = num(rest.next(), "--hole")? as f32,
            "--no-fade" => comp.fade = false,
            other => return Err(format!("unknown option {other}\n{usage}")),
        }
    }
    if comp.outline_mm != 0.0 || comp.hole_mm != 0.0 {
        options.bottom_compensation = Some(comp);
    }
    let start = std::time::Instant::now();
    let s = slice_stl_to_goo(
        Path::new(stl),
        Path::new(goo),
        &Machine::saturn_4_ultra_16k(),
        &Resin::siraya_fast_navy_grey(),
        &options,
    )?;
    println!(
        "{goo}: {} layers, print time {} s, {:.3} ml, {} g, sliced in {:.1} s",
        s.layers,
        s.print_time_s,
        s.volume_mm3 / 1000.0,
        s.weight_g,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
