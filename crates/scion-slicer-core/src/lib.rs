//! Scion Slicer's own code on top of the DragonFruit engine.
//!
//! The engine, its plugins and the material profiles come from DragonFruit, pinned in
//! `vendor/DragonFruit`. This crate only *uses* them; changes that have to live inside
//! the engine are carried as patches in `patches/dragonfruit/`.
//!
//! - [`checks`]: print checks (resin traps, suction cups, peel alerts, ledges).
//! - [`profiles`]: the printers and resin profiles to slice with.
//! - [`ranges`]: slicing by height ranges into a CTB file with per-layer settings.
//! - [`slice`]: slicing a mesh into a printer file.
//! - [`stl`]: loading meshes.

pub mod checks;
pub mod profiles;
pub mod ranges;
pub mod slice;
pub mod stl;

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use checks::{rasterize_for_checks, run_checks, ChecksConfig, Finding};

/// Default pixel size and layer height for the checks, in mm.
pub const DEFAULT_PX_MM: f64 = 0.05;
pub const DEFAULT_LAYER_HEIGHT_MM: f64 = 0.05;

/// The result of checking one mesh.
#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    pub layers: usize,
    pub layer_height_mm: f64,
    pub px_mm: f64,
    /// Size of the mesh's bounding box in mm (x, y, z).
    pub size_mm: [f64; 3],
    pub findings: Vec<Finding>,
    pub elapsed_ms: f64,
}

/// Run the print checks on an STL, in print orientation (lowest point on the plate).
pub fn check_stl(
    path: &Path,
    px_mm: f64,
    layer_height_mm: f64,
    config: &ChecksConfig,
) -> Result<CheckReport, String> {
    if !(px_mm > 0.0 && layer_height_mm > 0.0) {
        return Err("pixel size and layer height must be positive".into());
    }
    let start = Instant::now();
    let mesh = stl::load(path)?;
    let (masks, grid) = rasterize_for_checks(
        &mesh.triangles,
        mesh.bbox_min,
        mesh.bbox_max,
        px_mm,
        layer_height_mm,
    );
    let findings = run_checks(&masks, &grid, config);
    Ok(CheckReport {
        layers: masks.len(),
        layer_height_mm,
        px_mm,
        size_mm: [0, 1, 2].map(|i| mesh.bbox_max[i] - mesh.bbox_min[i]),
        findings,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}
