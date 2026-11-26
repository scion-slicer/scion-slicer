//! Write a small file and read it back.

use scion_slicer_ctb::{
    rle, write, CtbFile, FileSettings, Layer, LayerSettings, Machine, Motion, Preview, ResinInfo,
};

#[test]
fn written_files_read_back() {
    let machine = Machine {
        name: "Test".into(),
        resolution_x: 64,
        resolution_y: 32,
        display_width_mm: 6.4,
        display_height_mm: 3.2,
        machine_z_mm: 100.0,
    };
    let motion = Motion {
        lift_mm: 0.05,
        lift_speed_mm_min: 60.0,
        lift2_mm: 4.5,
        lift2_speed_mm_min: 240.0,
        retract_speed_mm_min: 240.0,
        retract2_mm: 2.0,
        retract2_speed_mm_min: 60.0,
    };
    let normal = LayerSettings {
        exposure_s: 3.1,
        rest_before_lift_s: 1.0,
        rest_after_lift_s: 1.5,
        rest_after_retract_s: 3.0,
        pwm: 255,
        motion,
        ..Default::default()
    };
    let tall = LayerSettings {
        exposure_s: 5.0,
        rest_before_lift_s: 2.0,
        rest_after_retract_s: 5.0,
        ..normal
    };
    let mut img = vec![0u8; 64 * 32];
    img[100..300].fill(0xFF);
    let layers: Vec<Layer> = (0..4)
        .map(|i| Layer {
            z_mm: 0.05 * (i + 1) as f32,
            settings: if i < 2 { normal } else { tall },
            data: rle::encode(&img),
        })
        .collect();
    let settings = FileSettings {
        layer_height_mm: 0.05,
        bottom_layers: 1,
        transition_layers: 0,
        bottom: normal,
        normal,
        per_layer_settings: true,
        anti_alias_level: 4,
        layer_xor_key: 0x1234_5678,
        modified_unix: 1_800_000_000,
        volume_ml: 1.0,
    };
    let resin = ResinInfo {
        resin_type: "Type".into(),
        resin_name: "Name".into(),
        profile_name: "Profile".into(),
        density_g_ml: 1.11,
        price_per_kg: 100.0,
        color: [255, 255, 255, 0],
    };
    let preview = Preview {
        width: 4,
        height: 2,
        rgb: vec![40; 24],
    };
    let (bytes, summary) = write(&machine, &resin, &settings, &preview, &layers).unwrap();
    assert_eq!(summary.bytes, bytes.len());

    let f = CtbFile::parse(&bytes).unwrap();
    assert!(f.signature_ok);
    assert_eq!(
        crc32fast::hash(&bytes[..bytes.len() - 4]).to_le_bytes(),
        bytes[bytes.len() - 4..]
    );
    assert_eq!(f.settings.u("PerLayerSettings"), 64);
    assert_eq!(f.settings.u("LayerCount"), 4);
    assert_eq!(f.machine_name, "Test");
    assert_eq!(f.layers[3].def.f("ExposureTime"), 5.0);
    assert_eq!(f.layers[3].def.f("RestTimeAfterRetract"), 5.0);
    assert_eq!(f.layers[0].def.f("LiftHeight"), 4.55);
    for l in &f.layers {
        assert_eq!(l.pixels(64 * 32).unwrap(), img);
    }
}
