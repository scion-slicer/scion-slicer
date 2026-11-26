//! Slice an STL by height ranges into an encrypted CTB file for the Saturn 4 Ultra 16K,
//! with the strip-set mold top's profile: 3DRS Platinum Cure Cast Clear at 50 µm up to
//! 4.00 mm, then 100 µm with a 5 s exposure and a 5 s rest after retract.
//!
//!   cargo run --release -p scion-slicer-core --example slice_ranges -- part.stl part.ctb \
//!       [--split MM] [--at X Y] [--rotate DEG] [--drop-blank] [--single] [--mirror none|x|y|xy]
//!
//! --split moves the range boundary (default 4.00 mm); --single slices it all at 50 µm with
//! the first range's settings and no per-layer flag, like Chitubox's baseline file.

use std::path::Path;

use scion_slicer_core::ranges::{slice_ranges_to_ctb, HeightRange, RangeProfile};
use scion_slicer_core::slice::{Placement, SliceOptions};
use scion_slicer_core::stl;
use scion_slicer_ctb::{LayerSettings, Motion, ResinInfo};
use scion_slicer_goo::Machine;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(stl_path), Some(out_path)) = (args.first(), args.get(1)) else {
        return Err("usage: slice_ranges part.stl part.ctb [--split MM] [--at X Y] [--rotate DEG] [--drop-blank] [--single]".into());
    };
    let mut options = SliceOptions::default();
    let (mut split, mut drop_blank, mut single) = (4.0, false, false);
    let mut machine = Machine::saturn_4_ultra_16k();
    let mut rest = args[2..].iter();
    let num = |v: Option<&String>| -> Result<f64, String> {
        v.ok_or("missing value")?
            .parse::<f64>()
            .map_err(|e| e.to_string())
    };
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--split" => split = num(rest.next())?,
            "--at" => {
                options.placement = Placement::Corner {
                    x_mm: num(rest.next())?,
                    y_mm: num(rest.next())?,
                }
            }
            "--rotate" => options.rotate_z_deg = num(rest.next())?,
            "--drop-blank" => drop_blank = true,
            "--single" => single = true,
            "--mirror" => {
                let m = rest.next().ok_or("--mirror needs none, x, y or xy")?;
                machine.mirror_x = m.contains('x');
                machine.mirror_y = m.contains('y');
            }
            other => return Err(format!("unknown option {other}")),
        }
    }

    // The motion and rests are the ones in Chitubox Pro's file for this profile.
    let normal_motion = Motion {
        lift_mm: 0.05,
        lift_speed_mm_min: 60.0,
        lift2_mm: 4.5,
        lift2_speed_mm_min: 240.0,
        retract_speed_mm_min: 240.0,
        retract2_mm: 2.0,
        retract2_speed_mm_min: 60.0,
    };
    let bottom_motion = Motion {
        lift_speed_mm_min: 45.0,
        lift2_mm: 4.0,
        ..normal_motion
    };
    let fine = LayerSettings {
        exposure_s: 3.1,
        light_off_s: 0.0,
        rest_before_lift_s: 1.0,
        rest_after_lift_s: 1.5,
        rest_after_retract_s: 3.0,
        pwm: 255,
        motion: normal_motion,
    };
    let coarse = LayerSettings {
        exposure_s: 5.0,
        rest_before_lift_s: 2.0,
        rest_after_retract_s: 5.0,
        ..fine
    };
    let ranges = if single {
        vec![HeightRange {
            up_to_mm: None,
            layer_height_mm: 0.05,
            settings: fine,
        }]
    } else {
        vec![
            HeightRange {
                up_to_mm: Some(split),
                layer_height_mm: 0.05,
                settings: fine,
            },
            HeightRange {
                up_to_mm: None,
                layer_height_mm: 0.1,
                settings: coarse,
            },
        ]
    };
    let profile = RangeProfile {
        bottom_layers: 5,
        transition_layers: 5,
        bottom: LayerSettings {
            motion: bottom_motion,
            ..fine
        },
        ranges,
        resin: ResinInfo {
            resin_type: "Platinum Silicone Casting".into(),
            resin_name: "3DRS Platinum Cure Cast Clear".into(),
            profile_name: "3DRS - Platinum Cure Cast Clear - 50um - On-Plate Normal".into(),
            density_g_ml: 1.11,
            price_per_kg: 144.1,
            color: [255, 255, 255, 0],
        },
        anti_alias_level: 4,
        drop_blank_last_layer: drop_blank,
    };
    let mesh = stl::load(Path::new(stl_path))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let start = std::time::Instant::now();
    let (bytes, report) = slice_ranges_to_ctb(&mesh, &machine, &profile, &options, now)?;
    std::fs::write(out_path, &bytes).map_err(|e| e.to_string())?;
    let last = report.layers.last().map(|l| l.z_mm).unwrap_or(0.0);
    println!(
        "{out_path}: {} layers to z {last}, {} bytes, print time {} s, {:.3} ml, {:.2} g, sliced in {:.1} s",
        report.layers.len(),
        report.bytes,
        report.print_time_s,
        report.volume_ml,
        report.weight_g,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
