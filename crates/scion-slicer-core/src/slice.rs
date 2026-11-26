//! Slicing a mesh into a printer file: the engine's rasterizer draws each layer at the
//! printer's own resolution, and `scion-slicer-goo` writes the `.goo` file.

use std::io::{Seek, Write};
use std::path::Path;

use dragonfruit_slicing_engine::geometry::{parse_triangles, project_triangles_inplace, Triangle};
use dragonfruit_slicing_engine::index::{build_layer_index, LayerSamplingSpan};
use dragonfruit_slicing_engine::raster::rasterize_layer;
use dragonfruit_slicing_engine::types::SliceJobV3;
use rayon::prelude::*;
use scion_slicer_goo::{prepare_layer, FileInfo, GooWriter, Machine, Resin, Summary};

use serde::Serialize;

use crate::profiles::{Catalog, Source};
use crate::stl::{self, Mesh};

/// Where the part goes on the build plate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Centred on the screen.
    Centered,
    /// The corner of the part's bounding box at this point, in mm from the corner of the
    /// screen where its first pixel is, as SatelLite places a part it loads.
    Corner { x_mm: f64, y_mm: f64 },
}

/// How to slice: where the part goes, how it's turned, and the bottom compensation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliceOptions {
    pub placement: Placement,
    /// Turn the part about the vertical axis by this many degrees (counter-clockwise seen
    /// from above) before placing it.
    pub rotate_z_deg: f64,
    pub bottom_compensation: Option<BottomCompensation>,
}

impl Default for SliceOptions {
    fn default() -> Self {
        Self {
            placement: Placement::Centered,
            rotate_z_deg: 0.0,
            bottom_compensation: None,
        }
    }
}

/// Elephant's-foot compensation on the layers against the plate, in mm: negative shrinks
/// outlines and opens holes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BottomCompensation {
    pub outline_mm: f32,
    pub hole_mm: f32,
    /// Fade it out over the transition layers, as the exposure steps down (as DragonFruit
    /// does by default); otherwise the transition layers get it in full.
    pub fade: bool,
}

/// How many layers to draw at once. Each one is a full-screen image while it's being
/// encoded (94 MB on the Saturn 4 Ultra 16K), so this bounds the memory used.
const LAYERS_PER_BATCH: usize = 64;

/// Slice an STL into a `.goo` file for `machine` with `resin`.
pub fn slice_stl_to_goo(
    stl_path: &Path,
    out_path: &Path,
    machine: &Machine,
    resin: &Resin,
    options: &SliceOptions,
) -> Result<Summary, String> {
    let mesh = stl::load(stl_path)?;
    let file =
        std::fs::File::create(out_path).map_err(|e| format!("{}: {e}", out_path.display()))?;
    let (_, summary) = slice_to_goo(
        &mesh,
        std::io::BufWriter::new(file),
        machine,
        resin,
        options,
    )?;
    Ok(summary)
}

/// Slice a mesh into a `.goo` file written to `out`.
pub fn slice_to_goo<W: Write + Seek>(
    mesh: &Mesh,
    out: W,
    machine: &Machine,
    resin: &Resin,
    options: &SliceOptions,
) -> Result<(W, Summary), String> {
    let job = screen_job(machine, resin, options.bottom_compensation);
    let (vertices, height) = place(mesh, machine, &job, options)?;
    let flat: Vec<f32> = vertices.iter().flat_map(|v| v.map(|c| c as f32)).collect();
    let mut projected = parse_triangles(&flat);
    project_triangles_inplace(&mut projected, &job);
    let total_layers = (height / resin.layer_height_mm as f64).ceil().max(0.0) as u32;
    let index = build_layer_index(
        &projected,
        total_layers,
        job.layer_height_mm,
        LayerSamplingSpan::CenterOnly,
    );

    let mut writer = GooWriter::new(
        out,
        machine.clone(),
        resin.clone(),
        FileInfo::scion_slicer(),
    )
    .map_err(|e| e.to_string())?;
    let layers: Vec<u32> = (0..total_layers).collect();
    for batch in layers.chunks(LAYERS_PER_BATCH) {
        let prepared = batch
            .par_iter()
            .map(|&l| {
                let pixels = rasterize_layer(&job, &projected, index.candidates_for_layer(l), l);
                prepare_layer(machine, &pixels)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for layer in &prepared {
            writer.add_prepared(layer).map_err(|e| e.to_string())?;
        }
    }
    writer
        .finish(Some(mesh_volume_mm3(&mesh.triangles)))
        .map_err(|e| e.to_string())
}

/// Turn the part and move it where `options` puts it, in the engine's coordinates for
/// `job` (pixel 0's edge at -build/2), lowest point on the plate. Returns the vertices,
/// three per triangle, and the part's height.
pub(crate) fn place(
    mesh: &Mesh,
    machine: &Machine,
    job: &SliceJobV3,
    options: &SliceOptions,
) -> Result<(Vec<[f64; 3]>, f64), String> {
    let pitch = pixel_pitch(machine);
    let (sin, cos) = options.rotate_z_deg.to_radians().sin_cos();
    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(mesh.triangles.len() * 3);
    for t in &mesh.triangles {
        for v in [t.a, t.b, t.c] {
            let (x, y) = (v.x as f64, v.y as f64);
            vertices.push([x * cos - y * sin, x * sin + y * cos, v.z as f64]);
        }
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in &vertices {
        for i in 0..3 {
            lo[i] = lo[i].min(v[i]);
            hi[i] = hi[i].max(v[i]);
        }
    }
    let size = [hi[0] - lo[0], hi[1] - lo[1]];
    let screen = [
        machine.resolution_x as f64 * pitch[0],
        machine.resolution_y as f64 * pitch[1],
    ];
    let corner = match options.placement {
        Placement::Centered => [(screen[0] - size[0]) / 2.0, (screen[1] - size[1]) / 2.0],
        Placement::Corner { x_mm, y_mm } => [x_mm, y_mm],
    };
    if corner[0] < 0.0
        || corner[1] < 0.0
        || corner[0] + size[0] > screen[0]
        || corner[1] + size[1] > screen[1]
    {
        return Err(format!(
            "the part ({:.2} x {:.2} mm) doesn't fit on the {}'s screen ({:.2} x {:.2} mm) there",
            size[0], size[1], machine.name, screen[0], screen[1]
        ));
    }
    // The engine puts pixel 0's edge at -build/2 and spreads the build over n - 1 pixels.
    let shift = [
        corner[0] - lo[0] - job.build_width_mm as f64 / 2.0,
        corner[1] - lo[1] - job.build_depth_mm as f64 / 2.0,
        -lo[2],
    ];
    for v in &mut vertices {
        for i in 0..3 {
            v[i] += shift[i];
        }
    }
    Ok((vertices, hi[2] - lo[2]))
}

/// The screen's pixel size in mm, across and down.
pub(crate) fn pixel_pitch(machine: &Machine) -> [f64; 2] {
    [
        machine.display_width_mm as f64 / machine.resolution_x as f64,
        machine.display_height_mm as f64 / machine.resolution_y as f64,
    ]
}

/// A job that draws the whole screen at the printer's resolution, black and white, with
/// pixel edges every `pitch` mm from the screen's corner (the engine spreads its build
/// size over n - 1 pixels, so the build is one pixel short of the screen).
fn screen_job(
    machine: &Machine,
    resin: &Resin,
    compensation: Option<BottomCompensation>,
) -> SliceJobV3 {
    screen_job_for(
        machine,
        resin.layer_height_mm,
        resin.bottom_layers,
        resin.transition_layers as u32,
        compensation,
    )
}

/// [`screen_job`] for any layer height and bottom and transition layer counts.
pub(crate) fn screen_job_for(
    machine: &Machine,
    layer_height_mm: f32,
    bottom: u32,
    transition: u32,
    compensation: Option<BottomCompensation>,
) -> SliceJobV3 {
    let pitch = pixel_pitch(machine);
    let (w, h) = (machine.resolution_x as u32, machine.resolution_y as u32);
    let c = compensation.unwrap_or(BottomCompensation {
        outline_mm: 0.0,
        hole_mm: 0.0,
        fade: true,
    });
    SliceJobV3 {
        bottom_outline_compensation_mm: c.outline_mm,
        bottom_hole_compensation_mm: c.hole_mm,
        bottom_compensation_layers: if c.fade { bottom } else { bottom + transition },
        bottom_compensation_fade_layers: if c.fade { transition } else { 0 },
        source_width_px: w,
        source_height_px: h,
        width_px: w,
        height_px: h,
        build_width_mm: ((w - 1) as f64 * pitch[0]) as f32,
        build_depth_mm: ((h - 1) as f64 * pitch[1]) as f32,
        layer_height_mm,
        total_layers: 0,
        anti_aliasing_level: "Off".to_string(),
        x_packing_mode: "none".to_string(),
        mirror_x: machine.mirror_x,
        mirror_y: machine.mirror_y,
        ..Default::default()
    }
}

/// The volume a closed mesh encloses, in mm³, from the signed volumes of its triangles.
pub fn mesh_volume_mm3(triangles: &[Triangle]) -> f64 {
    let v = triangles.iter().map(|t| {
        let (a, b, c) = (t.a, t.b, t.c);
        let (ax, ay, az) = (a.x as f64, a.y as f64, a.z as f64);
        let (bx, by, bz) = (b.x as f64, b.y as f64, b.z as f64);
        let (cx, cy, cz) = (c.x as f64, c.y as f64, c.z as f64);
        ax * (by * cz - bz * cy) - ay * (bx * cz - bz * cx) + az * (bx * cy - by * cx)
    });
    (v.sum::<f64>() / 6.0).abs()
}

/// What to slice and how, by name: what an agent asks for.
#[derive(Debug, Clone, Default)]
pub struct SliceRequest {
    pub stl_path: std::path::PathBuf,
    /// Default: the STL's path with a `.goo` extension.
    pub output_path: Option<std::path::PathBuf>,
    /// Printer name or part of it. Default: the first printer.
    pub printer: Option<String>,
    /// Resin profile name or part of it. Default: the profile SatelLite last used.
    pub resin: Option<String>,
    pub options: SliceOptions,
    /// Replace an existing output file.
    pub overwrite: bool,
}

/// What came out of a slice.
#[derive(Debug, Clone, Serialize)]
pub struct SliceReport {
    pub output_path: String,
    pub printer: String,
    pub resin: String,
    pub profile_source: Source,
    pub layers: u32,
    pub layer_height_mm: f32,
    pub print_time_s: u32,
    pub print_time: String,
    pub volume_ml: f64,
    pub weight_g: f32,
    pub cost: f32,
    pub rotate_z_deg: f64,
    pub bottom_compensation: Option<BottomCompensation>,
    pub elapsed_ms: f64,
    pub notes: Vec<String>,
}

/// Slice an STL with a printer and resin profile from `catalog`.
pub fn slice_stl(request: &SliceRequest, catalog: &Catalog) -> Result<SliceReport, String> {
    let start = std::time::Instant::now();
    let printer = catalog.printer(request.printer.as_deref())?;
    let resin = catalog.resin(printer, request.resin.as_deref())?;
    let out = request
        .output_path
        .clone()
        .unwrap_or_else(|| request.stl_path.with_extension("goo"));
    if out.exists() && !request.overwrite {
        return Err(format!(
            "{} already exists; pass overwrite to replace it",
            out.display()
        ));
    }
    let summary = slice_stl_to_goo(
        &request.stl_path,
        &out,
        &printer.machine,
        resin,
        &request.options,
    )?;

    let mut notes = vec![
        "Scion Slicer's .goo writer reproduces ELEGOO SatelLite's files exactly in its tests, but its \
         files are new: watch the first prints."
            .to_string(),
    ];
    if catalog.source == Source::BuiltIn {
        notes.push("ELEGOO SatelLite's profiles weren't found on this computer, so a built-in profile was used.".into());
    }
    if let Some(problem) = &catalog.problem {
        notes.push(format!("SatelLite's profiles couldn't be read: {problem}"));
    }
    let t = summary.print_time_s;
    Ok(SliceReport {
        output_path: out.display().to_string(),
        printer: printer.machine.name.clone(),
        resin: resin.profile_name.clone(),
        profile_source: catalog.source.clone(),
        layers: summary.layers,
        layer_height_mm: resin.layer_height_mm,
        print_time_s: t,
        print_time: format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60),
        volume_ml: summary.volume_mm3 as f64 / 1000.0,
        weight_g: summary.weight_g,
        cost: summary.cost,
        rotate_z_deg: request.options.rotate_z_deg,
        bottom_compensation: request.options.bottom_compensation,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        notes,
    })
}
