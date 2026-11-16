//! List the printers and resin profiles ELEGOO SatelLite has on this computer.
//!
//!   cargo run --release -p scion-slicer-goo --example profiles [machines.json]

use std::path::PathBuf;

use scion_slicer_goo::satellite::{default_machines_path, load_machines};

fn main() -> Result<(), String> {
    let path = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => default_machines_path().ok_or("SatelLite's machines.json wasn't found")?,
    };
    for printer in load_machines(&path)? {
        let m = &printer.machine;
        println!(
            "{}: {} x {} px, {} x {} x {} mm, mirror x {} y {}, tilt {} s a layer",
            m.name, m.resolution_x, m.resolution_y, m.display_width_mm, m.display_height_mm, m.machine_z_mm, m.mirror_x, m.mirror_y, m.tilt_seconds_per_layer
        );
        for r in &printer.resins {
            let active = if printer.active_resin.as_deref() == Some(&r.profile_name) { " (active)" } else { "" };
            println!(
                "  {}{active}: {} mm, {} bottom + {} transition, {} s / {} s, waits {:?} / {:?}, lift+retract {:.2} s, {} g/ml, {} /kg",
                r.profile_name,
                r.layer_height_mm,
                r.bottom_layers,
                r.transition_layers,
                r.bottom.exposure_s,
                r.normal.exposure_s,
                (r.bottom.waits.before_cure_s, r.bottom.waits.before_release_s, r.bottom.waits.after_release_s),
                (r.normal.waits.before_cure_s, r.normal.waits.before_release_s, r.normal.waits.after_release_s),
                r.normal.motion.seconds(),
                r.density_g_ml,
                r.price_per_kg
            );
        }
    }
    Ok(())
}
