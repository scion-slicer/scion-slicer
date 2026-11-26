//! Print everything in an encrypted CTB file.
//!
//!   cargo run --release -p scion-slicer-ctb --example ctb_dump -- file.ctb [--pixels]

use scion_slicer_ctb::CtbFile;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().ok_or("usage: ctb_dump file.ctb [--pixels]")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let f = CtbFile::parse(&bytes)?;
    println!(
        "{path}: {} bytes, signature {}",
        f.len,
        if f.signature_ok { "ok" } else { "DIFFERENT" }
    );
    let show = |title: &str, r: &scion_slicer_ctb::fields::Record| {
        println!(
            "{title}: {}",
            r.fields
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    show("header", &f.header);
    show("settings", &f.settings);
    show("large preview", &f.large_preview);
    show("small preview", &f.small_preview);
    println!("machine name: {:?}", f.machine_name);
    let n = f.pixel_count();
    for (i, l) in f.layers.iter().enumerate() {
        let d = &l.def;
        let mut line = format!(
            "L{i}: def@{} ptr {:?} z {} exp {} off {} data {}+{} enc {}+{} u3 {} lift {}@{} lift2 {}@{} ret {} ret2 {}@{} rest {}/{}/{} pwm {} u6 {}",
            l.def_offset, l.pointer, d.f("PositionZ"), d.f("ExposureTime"), d.f("LightOffDelay"),
            d.u("LayerDataOffset"), d.u("LayerDataLength"), d.u("EncryptedDataOffset"), d.u("EncryptedDataLength"), d.u("Unknown3"),
            d.f("LiftHeight"), d.f("LiftSpeed"), d.f("LiftHeight2"), d.f("LiftSpeed2"), d.f("RetractSpeed"),
            d.f("RetractHeight2"), d.f("RetractSpeed2"), d.f("RestTimeBeforeLift"), d.f("RestTimeAfterLift"),
            d.f("RestTimeAfterRetract"), d.f("LightPWM"), d.u("Unknown6"),
        );
        if args.iter().any(|a| a == "--pixels") {
            let px = l.pixels(n)?;
            let lit = px.iter().filter(|&&p| p != 0).count();
            let full = px.iter().filter(|&&p| p == 0xFF).count();
            line += &format!(" | lit {lit} full {full}");
        }
        println!("{line}");
    }
    Ok(())
}
