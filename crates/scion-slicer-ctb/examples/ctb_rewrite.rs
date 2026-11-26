//! Rewrite an encrypted CTB file with Scion Slicer's writer: the same images and the same
//! per-layer settings, read from the file. Comparing the result with the original (see
//! examples/compare) tests the writer on its own, apart from slicing.
//!
//!   cargo run --release -p scion-slicer-ctb --example ctb_rewrite -- theirs.ctb ours.ctb

use scion_slicer_ctb::fields::Record;
use scion_slicer_ctb::{
    write, CtbFile, FileSettings, Layer, LayerSettings, Machine, Motion, Preview, ResinInfo,
};

fn layer_settings(d: &Record) -> LayerSettings {
    LayerSettings {
        exposure_s: d.f("ExposureTime"),
        light_off_s: d.f("LightOffDelay"),
        rest_before_lift_s: d.f("RestTimeBeforeLift"),
        rest_after_lift_s: d.f("RestTimeAfterLift"),
        rest_after_retract_s: d.f("RestTimeAfterRetract"),
        pwm: d.f("LightPWM") as u8,
        motion: Motion {
            lift_mm: d.f("LiftHeight") - d.f("LiftHeight2"),
            lift_speed_mm_min: d.f("LiftSpeed"),
            lift2_mm: d.f("LiftHeight2"),
            lift2_speed_mm_min: d.f("LiftSpeed2"),
            retract_speed_mm_min: d.f("RetractSpeed"),
            retract2_mm: d.f("RetractHeight2"),
            retract2_speed_mm_min: d.f("RetractSpeed2"),
        },
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [src, dst] = &args[..] else {
        return Err("usage: ctb_rewrite theirs.ctb ours.ctb".into());
    };
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    let f = CtbFile::parse(&bytes)?;
    let s = &f.settings;
    let machine = Machine {
        name: f.machine_name.clone(),
        resolution_x: s.u("ResolutionX") as u32,
        resolution_y: s.u("ResolutionY") as u32,
        display_width_mm: s.f("DisplayWidth"),
        display_height_mm: s.f("DisplayHeight"),
        machine_z_mm: s.f("MachineZ"),
    };
    // Resin block: zeros, colour, three (offset, length) strings, density.
    let at = s.u("ResinParametersAddress") as usize;
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
    let text = |o: usize| {
        String::from_utf8_lossy(&bytes[u32_at(o)..u32_at(o) + u32_at(o + 4)]).into_owned()
    };
    let density = f32::from_le_bytes(bytes[at + 32..at + 36].try_into().unwrap());
    let grams = s.f("MaterialGrams");
    let resin = ResinInfo {
        resin_type: text(at + 8),
        resin_name: text(at + 16),
        profile_name: text(at + 24),
        density_g_ml: density,
        price_per_kg: s.f("MaterialCost") / grams * 1000.0,
        color: bytes[at + 4..at + 8].try_into().unwrap(),
    };
    let first = |i: usize| layer_settings(&f.layers[i].def);
    let bottom_layers = s.u("BottomLayerCount") as usize;
    let settings = FileSettings {
        layer_height_mm: s.f("LayerHeight"),
        bottom_layers: bottom_layers as u32,
        transition_layers: s.u("TransitionLayerCount") as u32,
        bottom: first(0),
        normal: first(bottom_layers.min(f.layers.len() - 1)),
        per_layer_settings: s.u("PerLayerSettings") != 0,
        anti_alias_level: s.u("AntiAliasLevel") as u32,
        layer_xor_key: s.u("LayerXorKey") as u32,
        modified_unix: s.u("ModifiedTimestampMinutes") * 60,
        volume_ml: s.f("MaterialMilliliters"),
    };
    let layers: Vec<Layer> = f
        .layers
        .iter()
        .map(|l| Layer {
            z_mm: l.def.f("PositionZ"),
            settings: layer_settings(&l.def),
            data: l.data.clone(),
        })
        .collect();
    let preview = Preview {
        width: 500,
        height: 210,
        rgb: vec![0x20; 500 * 210 * 3],
    };
    let (out, summary) = write(&machine, &resin, &settings, &preview, &layers)?;
    std::fs::write(dst, &out).map_err(|e| e.to_string())?;
    println!(
        "{dst}: {} layers, {} bytes, print time {} s (theirs {} s)",
        summary.layers,
        out.len(),
        summary.print_time_s,
        s.u("PrintTime")
    );
    Ok(())
}
