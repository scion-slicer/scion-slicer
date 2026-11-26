//! Slice an STL into a .ctb file with DragonFruit's own CTB encoder, as its Saturn 4 Ultra
//! 16K preset does (encrypted v5, "tilting" settings mode, mirror Y), with the strip-set
//! mold top's 3DRS Platinum Cure Cast Clear settings. Used to compare DragonFruit's output
//! with Chitubox's and Scion Slicer's.
//!
//!   cargo run --release -p scion-slicer-core --example df_ctb -- part.stl part.ctb

use std::path::Path;

use dragonfruit_slicing_engine::engine::slice_with_progress_v3;
use dragonfruit_slicing_engine::types::SliceJobV3;
use scion_slicer_core::stl;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [stl_path, out_path] = &args[..] else {
        return Err("usage: df_ctb part.stl part.ctb".into());
    };
    let mesh = stl::load(Path::new(stl_path))?;
    let (w, h) = (15120u32, 6230u32);
    // Centre the part on the plate and put its lowest point on it.
    let (cx, cy) = (
        (mesh.bbox_min[0] + mesh.bbox_max[0]) / 2.0,
        (mesh.bbox_min[1] + mesh.bbox_max[1]) / 2.0,
    );
    let mut xyz = Vec::with_capacity(mesh.triangles.len() * 9);
    for t in &mesh.triangles {
        for v in [t.a, t.b, t.c] {
            xyz.extend_from_slice(&[
                (v.x as f64 - cx) as f32,
                (v.y as f64 - cy) as f32,
                (v.z as f64 - mesh.bbox_min[2]) as f32,
            ]);
        }
    }
    let height = mesh.bbox_max[2] - mesh.bbox_min[2];
    let metadata = serde_json::json!({
        "machineName": "ELEGOO Saturn 4 Ultra 16K",
        "ctb": { "settingsMode": "tilting" },
        "material": {
            "layerHeightMm": 0.05,
            "bottomExposureSec": 3.1,
            "normalExposureSec": 3.1,
            "bottomLayerCount": 5,
            "transitionLayerCount": 5,
            "bottomWaitTimeBeforeCureSec": 3.0,
            "bottomWaitTimeAfterCureSec": 1.0,
            "bottomWaitTimeAfterLiftSec": 1.5,
            "waitTimeBeforeCureSec": 3.0,
            "waitTimeAfterCureSec": 1.0,
            "waitTimeAfterLiftSec": 1.5,
            "bottomLiftDistanceMm": 0.05,
            "bottomLiftDistance2Mm": 4.0,
            "bottomLiftSpeedMmMin": 45.0,
            "bottomLiftSpeed2MmMin": 240.0,
            "liftDistanceMm": 0.05,
            "liftDistance2Mm": 4.5,
            "liftSpeedMmMin": 60.0,
            "liftSpeed2MmMin": 240.0,
            "retractSpeedMmMin": 240.0,
            "retractDistance2Mm": 2.0,
            "retractSpeed2MmMin": 60.0,
            "bottomRetractSpeedMmMin": 240.0,
            "bottomRetractSpeed2MmMin": 60.0,
            "bottomRetractHeight2Mm": 2.0,
            "projectorPwmPercent": 100,
            "bottomProjectorPwmPercent": 100
        }
    });
    let job = SliceJobV3 {
        output_format: ".ctb".into(),
        format_version: Some("v5enc".into()),
        source_width_px: w,
        source_height_px: h,
        width_px: w,
        height_px: h,
        build_width_mm: 211.68,
        build_depth_mm: 118.37,
        layer_height_mm: 0.05,
        total_layers: (height / 0.05).ceil() as u32,
        anti_aliasing_level: "Off".into(),
        x_packing_mode: "none".into(),
        mirror_x: false,
        mirror_y: true,
        triangles_xyz: xyz,
        model_triangle_count: mesh.triangles.len() as u32,
        metadata_json: metadata.to_string(),
        ..Default::default()
    };
    let artifact = slice_with_progress_v3(&job, None, None).map_err(|e| format!("{e:?}"))?;
    std::fs::write(out_path, &artifact.bytes).map_err(|e| e.to_string())?;
    println!(
        "{out_path}: {} layers, {} bytes",
        job.total_layers,
        artifact.bytes.len()
    );
    Ok(())
}
