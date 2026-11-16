//! Rewrite SatelLite's own reference slices with our writer and compare.
//!
//! For each file in `corpus/satellite/saturn-4-ultra-16k`: decode SatelLite's layers, write
//! them again with the settings the file was sliced with, and require every layer record,
//! every layer's data, every header field and the footer to come out the same. Only the
//! preview images and the bytes SatelLite leaves after the end of its text fields may
//! differ. The settings come from the corpus README, not from the files, so a wrong rule
//! in the writer can't hide behind a value copied out of SatelLite's header.

use std::io::Cursor;
use std::path::PathBuf;

use scion_slicer_goo::rle::decode_layer;
use scion_slicer_goo::{FileInfo, GooFile, GooWriter, LayerSettings, Machine, Motion, Resin, Waits};

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/satellite/saturn-4-ultra-16k")
}

/// SatelLite's default resin with the SirayaTech Fast (Navy Grey) values typed in.
fn typed_in() -> Resin {
    Resin {
        profile_name: "Normal".into(),
        layer_height_mm: 0.05,
        bottom_layers: 5,
        transition_layers: 10,
        bottom: LayerSettings {
            exposure_s: 30.0,
            waits: Waits { before_cure_s: 1.0, before_release_s: 1.5, after_release_s: 0.0 },
            pwm: 255,
            motion: Motion::default(),
        },
        normal: LayerSettings {
            exposure_s: 1.9,
            waits: Waits { before_cure_s: 0.0, before_release_s: 0.0, after_release_s: 1.0 },
            pwm: 255,
            motion: Motion::default(),
        },
        density_g_ml: 1.15,
        price_per_kg: 30.0,
        currency: "$".into(),
    }
}

/// The SirayaTech Fast (Navy Grey) profile itself, with the lift and retract it keeps
/// out of sight: 5 mm at 65 mm/min up, 5 mm at 150 mm/min back.
fn siraya() -> Resin {
    let motion = Motion { lift_mm: 5.0, lift_speed_mm_min: 65.0, retract_mm: 5.0, retract_speed_mm_min: 150.0 };
    let mut r = typed_in();
    r.profile_name = "SirayaTech - Fast (Navy Grey)".into();
    r.bottom.motion = motion;
    r.normal.motion = motion;
    r
}

fn cases() -> Vec<(&'static str, Resin, f64)> {
    let with = |f: fn(&mut Resin)| {
        let mut r = typed_in();
        f(&mut r);
        r
    };
    vec![
        ("00-small-baseline", typed_in(), 800.0),
        ("02-tall", typed_in(), 4000.0),
        ("03-wide", typed_in(), 27000.0),
        ("04-small-transition-5", with(|r| r.transition_layers = 5), 800.0),
        ("05-small-transition-0", with(|r| r.transition_layers = 0), 800.0),
        ("06-small-bottom-3", with(|r| r.bottom_layers = 3), 800.0),
        ("07-small-bottom-exposure-20", with(|r| r.bottom.exposure_s = 20.0), 800.0),
        (
            "08-small-waits",
            with(|r| {
                r.bottom.waits = Waits { before_cure_s: 2.0, before_release_s: 3.0, after_release_s: 4.0 };
                r.normal.waits = Waits { before_cure_s: 0.5, before_release_s: 0.6, after_release_s: 0.7 };
            }),
            800.0,
        ),
        ("10-wide-siraya-profile", siraya(), 27000.0),
        ("11-small-siraya-profile", siraya(), 800.0),
    ]
}

#[test]
fn rewrites_satellite_files_identically() {
    let dir = corpus();
    if !dir.exists() {
        eprintln!("skipping: no reference slices at {}", dir.display());
        return;
    }
    let machine = Machine::saturn_4_ultra_16k();
    let mut failures = Vec::new();
    for (name, resin, volume) in cases() {
        let original = std::fs::read(dir.join(format!("{name}.goo"))).unwrap();
        let theirs = GooFile::parse(&original).unwrap();
        let info = FileInfo {
            software: theirs.header.software.clone(),
            version: theirs.header.software_version.clone(),
            created: theirs.header.created.clone(),
        };

        let mut writer = GooWriter::new(Cursor::new(Vec::new()), machine.clone(), resin, info).unwrap();
        for layer in &theirs.layers {
            writer.add_layer(&decode_layer(layer.data, machine.pixel_count()).unwrap()).unwrap();
        }
        let (out, summary) = writer.finish(Some(volume)).unwrap();
        let rewritten = out.into_inner();
        let ours = GooFile::parse(&rewritten).unwrap();

        if ours.header != theirs.header {
            failures.push(format!("{name}: header differs\n  ours:   {:?}\n  theirs: {:?}", ours.header, theirs.header));
        }
        for (i, (a, b)) in ours.layers.iter().zip(&theirs.layers).enumerate() {
            if a.record != b.record {
                failures.push(format!("{name}: layer {i} record differs\n  ours:   {a:?}\n  theirs: {b:?}", a = Rec(a), b = Rec(b)));
                break;
            }
            if a.data != b.data {
                failures.push(format!("{name}: layer {i} data differs"));
                break;
            }
        }
        if ours.footer != theirs.footer {
            failures.push(format!("{name}: footer differs"));
        }
        eprintln!("{name}: {} layers, {} s, {} g", summary.layers, summary.print_time_s, summary.weight_g);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn volume_falls_back_to_counting_pixels() {
    let path = corpus().join("00-small-baseline.goo");
    if !path.exists() {
        return;
    }
    let machine = Machine::saturn_4_ultra_16k();
    let original = std::fs::read(path).unwrap();
    let theirs = GooFile::parse(&original).unwrap();
    let mut writer = GooWriter::new(Cursor::new(Vec::new()), machine.clone(), typed_in(), FileInfo::scion_slicer()).unwrap();
    for layer in &theirs.layers {
        writer.add_layer(&decode_layer(layer.data, machine.pixel_count()).unwrap()).unwrap();
    }
    let (_, summary) = writer.finish(None).unwrap();
    // 20 x 20 x 2 mm: the pixels give 800 mm³ within a pixel's width around the edge.
    assert!((summary.volume_mm3 - 800.0).abs() < 8.0, "volume {}", summary.volume_mm3);
}

/// Prints a layer record's fields instead of its bytes.
struct Rec<'a, 'b>(&'a scion_slicer_goo::read::Layer<'b>);

impl std::fmt::Debug for Rec<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let l = self.0;
        write!(
            f,
            "pause {} pauseZ {} z {} exposure {} off {} waits {:?} motion {:?} pwm {}",
            l.pause, l.pause_position_z_mm, l.z_mm, l.exposure_s, l.light_off_s, l.waits_s, l.motion, l.pwm
        )
    }
}

/// The SirayaTech Fast (Navy Grey) profile read from this computer's SatelLite must be the
/// one that reproduced SatelLite's slices above.
#[test]
fn satellite_profile_matches_the_reference_settings() {
    let Some(path) = scion_slicer_goo::satellite::default_machines_path() else {
        eprintln!("skipping: SatelLite isn't installed here");
        return;
    };
    let printers = scion_slicer_goo::satellite::load_machines(&path).unwrap();
    let Some(printer) = printers.iter().find(|p| p.machine == Machine::saturn_4_ultra_16k()) else {
        eprintln!("skipping: no Saturn 4 Ultra 16K in SatelLite");
        return;
    };
    let Some(navy) = printer.resins.iter().find(|r| r.profile_name == "SirayaTech - Fast (Navy Grey)") else {
        eprintln!("skipping: no SirayaTech Fast (Navy Grey) profile");
        return;
    };
    assert_eq!(*navy, siraya());
}
