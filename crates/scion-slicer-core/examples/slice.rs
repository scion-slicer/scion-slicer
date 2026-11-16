//! Slice an STL into a `.goo` file for the Saturn 4 Ultra 16K with the SirayaTech Fast
//! (Navy Grey) settings.
//!
//!   cargo run --release -p scion-slicer-core --example slice -- part.stl part.goo [x_mm y_mm]
//!
//! Without a position the part is centred; with one, the corner of its bounding box goes
//! that far from the corner of the screen.

use std::path::Path;

use scion_slicer_core::slice::{slice_stl_to_goo, Placement};
use scion_slicer_goo::{LayerSettings, Machine, Motion, Resin, Waits};

fn siraya_fast_navy_grey() -> Resin {
    let motion = Motion { lift_mm: 5.0, lift_speed_mm_min: 65.0, retract_mm: 5.0, retract_speed_mm_min: 150.0 };
    Resin {
        profile_name: "SirayaTech - Fast (Navy Grey)".into(),
        layer_height_mm: 0.05,
        bottom_layers: 5,
        transition_layers: 10,
        bottom: LayerSettings {
            exposure_s: 30.0,
            waits: Waits { before_cure_s: 1.0, before_release_s: 1.5, after_release_s: 0.0 },
            pwm: 255,
            motion,
        },
        normal: LayerSettings {
            exposure_s: 1.9,
            waits: Waits { before_cure_s: 0.0, before_release_s: 0.0, after_release_s: 1.0 },
            pwm: 255,
            motion,
        },
        density_g_ml: 1.15,
        price_per_kg: 30.0,
        currency: "$".into(),
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let placement = match &args[..] {
        [_, _] => Placement::Centered,
        [_, _, x, y] => Placement::Corner {
            x_mm: x.parse().map_err(|e| format!("x: {e}"))?,
            y_mm: y.parse().map_err(|e| format!("y: {e}"))?,
        },
        _ => return Err("usage: slice part.stl part.goo [x_mm y_mm]".into()),
    };
    let start = std::time::Instant::now();
    let s = slice_stl_to_goo(
        Path::new(&args[0]),
        Path::new(&args[1]),
        &Machine::saturn_4_ultra_16k(),
        &siraya_fast_navy_grey(),
        placement,
    )?;
    println!(
        "{}: {} layers, print time {} s, {:.3} ml, {} g, sliced in {:.1} s",
        args[1],
        s.layers,
        s.print_time_s,
        s.volume_mm3 / 1000.0,
        s.weight_g,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
