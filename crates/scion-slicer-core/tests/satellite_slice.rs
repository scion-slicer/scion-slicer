//! Slice the reference boxes ourselves and compare with SatelLite's slices of them:
//! same layer count and the same pixels on every layer.

use std::io::Cursor;
use std::path::PathBuf;

use scion_slicer_core::slice::{slice_to_goo, Placement, SliceOptions};
use scion_slicer_core::stl;
use scion_slicer_goo::rle::decode_layer;
use scion_slicer_goo::{GooFile, LayerSettings, Machine, Motion, Resin, Waits};

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/satellite/saturn-4-ultra-16k")
}

fn typed_in() -> Resin {
    Resin {
        profile_name: "Normal".into(),
        layer_height_mm: 0.05,
        bottom_layers: 5,
        transition_layers: 10,
        bottom: LayerSettings {
            exposure_s: 30.0,
            waits: Waits {
                before_cure_s: 1.0,
                before_release_s: 1.5,
                after_release_s: 0.0,
            },
            pwm: 255,
            motion: Motion::default(),
        },
        normal: LayerSettings {
            exposure_s: 1.9,
            waits: Waits {
                before_cure_s: 0.0,
                before_release_s: 0.0,
                after_release_s: 1.0,
            },
            pwm: 255,
            motion: Motion::default(),
        },
        density_g_ml: 1.15,
        price_per_kg: 30.0,
        currency: "$".into(),
    }
}

#[test]
fn boxes_slice_to_satellites_pixels() {
    let dir = corpus();
    if !dir.exists() {
        eprintln!("skipping: no reference slices at {}", dir.display());
        return;
    }
    let machine = Machine::saturn_4_ultra_16k();
    for (model, reference) in [("small", "00-small-baseline"), ("wide", "03-wide")] {
        let mesh = stl::load(&dir.join(format!("{model}.stl"))).unwrap();
        // SatelLite left each box where it was loaded: its corner on the screen's corner.
        let options = SliceOptions {
            placement: Placement::Corner {
                x_mm: 0.0,
                y_mm: 0.0,
            },
            ..Default::default()
        };
        let (out, summary) = slice_to_goo(
            &mesh,
            Cursor::new(Vec::new()),
            &machine,
            &typed_in(),
            &options,
        )
        .unwrap();
        let ours_bytes = out.into_inner();
        let theirs_bytes = std::fs::read(dir.join(format!("{reference}.goo"))).unwrap();
        let (ours, theirs) = (
            GooFile::parse(&ours_bytes).unwrap(),
            GooFile::parse(&theirs_bytes).unwrap(),
        );

        assert_eq!(
            ours.layers.len(),
            theirs.layers.len(),
            "{model}: layer count"
        );
        assert_eq!(
            ours.header.print_time_s, theirs.header.print_time_s,
            "{model}: print time"
        );
        assert_eq!(
            ours.header.volume_mm3, theirs.header.volume_mm3,
            "{model}: volume"
        );
        assert_eq!(
            ours.header.weight_g, theirs.header.weight_g,
            "{model}: weight"
        );
        for (i, (a, b)) in ours.layers.iter().zip(&theirs.layers).enumerate() {
            if a.data != b.data {
                let pa = decode_layer(a.data, machine.pixel_count()).unwrap();
                let pb = decode_layer(b.data, machine.pixel_count()).unwrap();
                let differ = pa.iter().zip(&pb).filter(|(x, y)| x != y).count();
                let lit = |p: &[u8]| p.iter().filter(|&&v| v != 0).count();
                panic!(
                    "{model}: layer {i} differs in {differ} pixels (ours {} lit, SatelLite's {} lit)",
                    lit(&pa),
                    lit(&pb)
                );
            }
            assert_eq!(a.record, b.record, "{model}: layer {i} record");
        }
        eprintln!(
            "{model}: {} layers identical to SatelLite's, {} s",
            summary.layers, summary.print_time_s
        );
    }
}
