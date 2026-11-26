//! Slicing by height ranges: each range of heights gets its own layer height, exposure,
//! rests and motion, written as an encrypted CTB (v5) file with per-layer settings, the
//! way Chitubox Pro's "Slicing by Height" does.
//!
//! Rules taken from Chitubox Pro's own files (see docs/design/ctb-ranges.md):
//! - A range starts where the one below ends; its first layer's top is one of its layer
//!   heights above that (4.00 then 4.10 at 0.1 mm).
//! - A layer's z is `n × h` in 32-bit floats, where `n` counts layer heights `h` from the
//!   plate: 4.2000003 for the second 0.1 mm layer above 4.00, as Chitubox writes it.
//! - The last range has as many layers as it takes to reach the part's top, rounded up, so
//!   a part ending half way through a layer gets that layer, sliced at its middle. When the
//!   middle is at or above the top, the layer is blank, as Chitubox leaves it.
//! - Bottom and transition layers exist only in the first range. Transition layers step
//!   their exposure down from the bottom exposure to the first range's in equal steps and
//!   otherwise use the first range's settings.
//! - Images are mirrored left-right (Chitubox's convention for this printer; SatelLite's
//!   .goo files mirror top-bottom instead), and a centred part sits where Chitubox centres
//!   it ([`Placement::ChituboxCentered`]).

use dragonfruit_slicing_engine::geometry::{parse_triangles, project_triangles_inplace};
use dragonfruit_slicing_engine::index::{build_layer_index, LayerSamplingSpan};
use dragonfruit_slicing_engine::raster::rasterize_layer;
use rayon::prelude::*;
use scion_slicer_ctb as ctb;
use scion_slicer_goo::Machine;
use serde::Serialize;

use crate::slice::{pixel_pitch, place, screen_job_for, Placement, SliceOptions};
use crate::stl::Mesh;

/// A range of heights with its own settings.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightRange {
    /// Where the range ends, in mm from the plate. `None` for the last range: to the top.
    pub up_to_mm: Option<f64>,
    pub layer_height_mm: f64,
    pub settings: ctb::LayerSettings,
}

/// A profile sliced by height.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeProfile {
    pub bottom_layers: u32,
    pub transition_layers: u32,
    pub bottom: ctb::LayerSettings,
    /// At least one; the first holds the bottom and transition layers.
    pub ranges: Vec<HeightRange>,
    pub resin: ctb::ResinInfo,
    pub anti_alias_level: u32,
    /// Leave out a blank last layer instead of writing it as Chitubox does.
    pub drop_blank_last_layer: bool,
    /// Light a pixel by how much of it the part covers instead of by its centre: the
    /// engine's anti-aliasing level (e.g. "16x") measures the coverage, and a pixel is lit
    /// when its coverage (0-255) is above the threshold. `None` samples the centre only.
    pub coverage: Option<Coverage>,
}

/// Lighting pixels by coverage; see [`RangeProfile::coverage`].
#[derive(Debug, Clone, PartialEq)]
pub struct Coverage {
    pub level: String,
    pub threshold: u8,
}

/// One layer of the plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedLayer {
    pub index: u32,
    pub range: usize,
    /// Its top, as written to the file.
    pub z_mm: f32,
    pub layer_height_mm: f64,
    /// Where it's sliced: the middle of the layer, in mm from the plate.
    pub sample_z_mm: f64,
    #[serde(skip)]
    pub settings: ctb::LayerSettings,
    /// Its index within its range.
    #[serde(skip)]
    pub local: u32,
}

/// Lay out the layers of a part `height_mm` tall.
pub fn plan(height_mm: f64, p: &RangeProfile) -> Result<Vec<PlannedLayer>, String> {
    if p.ranges.is_empty() {
        return Err("a profile needs at least one height range".into());
    }
    let mut out = Vec::new();
    let mut start = 0.0f64;
    for (r, range) in p.ranges.iter().enumerate() {
        let h = range.layer_height_mm;
        if h.is_nan() || h <= 0.0 {
            return Err(format!("range {} has no layer height", r + 1));
        }
        let last = r + 1 == p.ranges.len() || range.up_to_mm.is_none_or(|top| top >= height_mm);
        let end = if last {
            height_mm
        } else {
            range.up_to_mm.unwrap()
        };
        if end <= start + 1e-9 {
            if last {
                break;
            }
            return Err(format!(
                "range {} ends at {end} mm, below where it starts ({start} mm)",
                r + 1
            ));
        }
        let span = (end - start) / h;
        let count = if last {
            (span - 1e-6).ceil()
        } else {
            span.round().max(1.0)
        } as u32;
        let units = start / h;
        let on_grid = (units - units.round()).abs() < 1e-6;
        for k in 0..count {
            let index = out.len() as u32;
            let z_mm = if on_grid {
                ((units.round() + (k + 1) as f64) * (h as f32) as f64) as f32
            } else {
                (start + (k + 1) as f64 * h) as f32
            };
            let settings = if r == 0 && index < p.bottom_layers {
                p.bottom
            } else if r == 0 && index < p.bottom_layers + p.transition_layers {
                let step = (p.bottom.exposure_s - range.settings.exposure_s) as f64
                    / p.transition_layers as f64;
                let k = (index - p.bottom_layers + 1) as f64;
                ctb::LayerSettings {
                    exposure_s: (p.bottom.exposure_s as f64 - step * k) as f32,
                    ..range.settings
                }
            } else {
                range.settings
            };
            out.push(PlannedLayer {
                index,
                range: r,
                z_mm,
                layer_height_mm: h,
                sample_z_mm: start + (k as f64 + 0.5) * h,
                settings,
                local: k,
            });
        }
        start += count as f64 * h;
        if last {
            break;
        }
    }
    Ok(out)
}

/// What came out of slicing by height.
#[derive(Debug, Clone, Serialize)]
pub struct RangeSliceReport {
    pub layers: Vec<PlannedLayer>,
    pub lit_pixels: Vec<u64>,
    pub print_time_s: u32,
    pub volume_ml: f32,
    pub weight_g: f32,
    pub bytes: usize,
}

/// Slice a mesh by height ranges into an encrypted CTB file.
pub fn slice_ranges_to_ctb(
    mesh: &Mesh,
    machine: &Machine,
    profile: &RangeProfile,
    options: &SliceOptions,
    modified_unix: u64,
) -> Result<(Vec<u8>, RangeSliceReport), String> {
    // Chitubox mirrors CTB images left-right where SatelLite mirrors .goo images top-bottom
    // (Chitubox's own .goo export of the mold top says mirror X too). The two differ by a
    // half turn, so the part prints the same way round either way; follow Chitubox here.
    let machine = &Machine {
        mirror_x: machine.mirror_x || machine.mirror_y,
        mirror_y: false,
        ..machine.clone()
    };
    // Centre the part where Chitubox does, so the layers match its pixel for pixel.
    let chitubox_options;
    let options = if options.placement == Placement::Centered {
        chitubox_options = SliceOptions {
            placement: Placement::ChituboxCentered,
            ..*options
        };
        &chitubox_options
    } else {
        options
    };
    let first_h = profile.ranges[0].layer_height_mm;
    let base_job = screen_job_for(
        machine,
        first_h as f32,
        profile.bottom_layers,
        profile.transition_layers,
        None,
    );
    let (vertices, height) = place(mesh, machine, &base_job, options)?;
    let mut planned = plan(height, profile)?;

    // Rasterize range by range: shift the part down to the range's start and slice it at
    // the range's layer height, so layer k is sampled at start + (k + 0.5) × h.
    let pixel_count = machine.pixel_count();
    let mut encoded: Vec<Option<(Vec<u8>, u64)>> = vec![None; planned.len()];
    let mut start = 0.0f64;
    for r in 0..profile.ranges.len() {
        let in_range: Vec<&PlannedLayer> = planned.iter().filter(|l| l.range == r).collect();
        let Some(first) = in_range.first() else { break };
        let h = first.layer_height_mm;
        let compensation = if r == 0 {
            options.bottom_compensation
        } else {
            None
        };
        let mut job = screen_job_for(
            machine,
            h as f32,
            profile.bottom_layers,
            profile.transition_layers,
            compensation,
        );
        if let Some(c) = &profile.coverage {
            job.anti_aliasing_level = c.level.clone();
        }
        let flat: Vec<f32> = vertices
            .iter()
            .flat_map(|v| [v[0] as f32, v[1] as f32, (v[2] - start) as f32])
            .collect();
        let mut tris = parse_triangles(&flat);
        project_triangles_inplace(&mut tris, &job);
        let n = in_range.len() as u32;
        let index = build_layer_index(&tris, n, job.layer_height_mm, LayerSamplingSpan::CenterOnly);
        let results: Vec<(u32, Vec<u8>, u64)> = in_range
            .par_iter()
            .map(|l| {
                let mut px =
                    rasterize_layer(&job, &tris, index.candidates_for_layer(l.local), l.local);
                if let Some(c) = &profile.coverage {
                    for p in &mut px {
                        *p = if *p > c.threshold { 0xFF } else { 0 };
                    }
                }
                debug_assert_eq!(px.len(), pixel_count);
                let lit = px.iter().filter(|&&p| p != 0).count() as u64;
                (l.index, ctb::rle::encode(&px), lit)
            })
            .collect();
        for (i, data, lit) in results {
            encoded[i as usize] = Some((data, lit));
        }
        start += n as f64 * h;
    }
    let mut layers: Vec<(PlannedLayer, Vec<u8>, u64)> = planned
        .drain(..)
        .zip(encoded)
        .map(|(p, e)| {
            let (d, lit) = e.expect("every planned layer is sliced");
            (p, d, lit)
        })
        .collect();
    if profile.drop_blank_last_layer {
        while layers.last().is_some_and(|(_, _, lit)| *lit == 0) {
            layers.pop();
        }
    }

    let pitch = pixel_pitch(machine);
    let volume_mm3: f64 = layers
        .iter()
        .map(|(p, _, lit)| *lit as f64 * pitch[0] * pitch[1] * p.layer_height_mm)
        .sum();
    let ctb_machine = ctb::Machine {
        name: machine.name.clone(),
        resolution_x: machine.resolution_x as u32,
        resolution_y: machine.resolution_y as u32,
        display_width_mm: machine.display_width_mm,
        display_height_mm: machine.display_height_mm,
        machine_z_mm: machine.machine_z_mm,
    };
    let first_settings = profile.ranges[0].settings;
    let per_layer = layers.iter().any(|(p, _, _)| p.range > 0);
    let settings = ctb::FileSettings {
        layer_height_mm: first_h as f32,
        bottom_layers: profile.bottom_layers,
        transition_layers: profile.transition_layers,
        bottom: profile.bottom,
        normal: first_settings,
        per_layer_settings: per_layer,
        anti_alias_level: profile.anti_alias_level,
        layer_xor_key: (modified_unix as u32).wrapping_mul(2_654_435_761) | 1,
        modified_unix,
        volume_ml: (volume_mm3 / 1000.0) as f32,
    };
    let preview = top_view(
        machine,
        &layers
            .iter()
            .map(|(_, d, _)| d.as_slice())
            .collect::<Vec<_>>(),
    );
    let ctb_layers: Vec<ctb::Layer> = layers
        .iter()
        .map(|(p, d, _)| ctb::Layer {
            z_mm: p.z_mm,
            settings: p.settings,
            data: d.clone(),
        })
        .collect();
    let (bytes, summary) = ctb::write(
        &ctb_machine,
        &profile.resin,
        &settings,
        &preview,
        &ctb_layers,
    )?;
    let report = RangeSliceReport {
        lit_pixels: layers.iter().map(|(_, _, lit)| *lit).collect(),
        layers: layers.into_iter().map(|(p, _, _)| p).collect(),
        print_time_s: summary.print_time_s,
        volume_ml: summary.volume_ml,
        weight_g: summary.weight_g,
        bytes: bytes.len(),
    };
    Ok((bytes, report))
}

/// A 500 × 210 top view of where any layer is lit, for the file's preview.
fn top_view(machine: &Machine, layers: &[&[u8]]) -> ctb::Preview {
    const W: usize = 500;
    const H: usize = 210;
    let (sw, sh) = (machine.resolution_x as usize, machine.resolution_y as usize);
    let mut lit = vec![false; W * H];
    for data in layers {
        // Walk the runs and mark the preview cells they cross on sampled rows.
        let mut pos = 0usize;
        let mut i = 0usize;
        while i < data.len() {
            let head = data[i];
            i += 1;
            let mut len = 1usize;
            if head & 0x80 != 0 {
                let b0 = data[i] as usize;
                i += 1;
                let (extra, top) = match b0 {
                    0x00..=0x7F => (0, b0),
                    0x80..=0xBF => (1, b0 & 0x3F),
                    0xC0..=0xDF => (2, b0 & 0x1F),
                    _ => (3, b0 & 0x0F),
                };
                len = top;
                for _ in 0..extra {
                    len = (len << 8) | data[i] as usize;
                    i += 1;
                }
            }
            if head & 0x7F != 0 {
                let (mut p, end) = (pos, pos + len);
                while p < end {
                    let (y, x) = (p / sw, p % sw);
                    let row_end = (y + 1) * sw;
                    let stop = end.min(row_end);
                    let cy = y * H / sh;
                    if y == cy * sh / H {
                        for cx in (x * W / sw)..=((stop - 1 - y * sw) * W / sw) {
                            lit[cy * W + cx] = true;
                        }
                    }
                    p = stop;
                }
            }
            pos += len;
        }
    }
    let mut rgb = Vec::with_capacity(W * H * 3);
    for y in 0..H {
        for x in 0..W {
            // Show it as seen from above: undo the printer's mirroring.
            let fx = if machine.mirror_x { W - 1 - x } else { x };
            let fy = if machine.mirror_y { H - 1 - y } else { y };
            rgb.extend_from_slice(if lit[fy * W + fx] {
                &[0x7D, 0x8B, 0xF0]
            } else {
                &[0x10, 0x10, 0x12]
            });
        }
    }
    ctb::Preview {
        width: W as u32,
        height: H as u32,
        rgb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(exposure_s: f32) -> ctb::LayerSettings {
        ctb::LayerSettings {
            exposure_s,
            ..Default::default()
        }
    }

    fn mold_top_profile() -> RangeProfile {
        RangeProfile {
            bottom_layers: 5,
            transition_layers: 5,
            bottom: settings(3.1),
            ranges: vec![
                HeightRange {
                    up_to_mm: Some(4.0),
                    layer_height_mm: 0.05,
                    settings: settings(3.1),
                },
                HeightRange {
                    up_to_mm: None,
                    layer_height_mm: 0.1,
                    settings: settings(5.0),
                },
            ],
            resin: ctb::ResinInfo {
                resin_type: String::new(),
                resin_name: String::new(),
                profile_name: String::new(),
                density_g_ml: 1.11,
                price_per_kg: 144.1,
                color: [255, 255, 255, 0],
            },
            anti_alias_level: 4,
            drop_blank_last_layer: false,
            coverage: None,
        }
    }

    #[test]
    fn plans_the_mold_top_as_chitubox_does() {
        let layers = plan(5.75, &mold_top_profile()).unwrap();
        assert_eq!(layers.len(), 98);
        assert_eq!(layers[79].z_mm, 4.0);
        assert_eq!(layers[80].z_mm, 4.1);
        assert_eq!(layers[81].z_mm.to_bits(), 4.200_000_3_f32.to_bits());
        assert_eq!(layers[97].z_mm, 5.8);
        assert_eq!(layers[97].sample_z_mm, 5.75);
        assert_eq!(layers[80].settings.exposure_s, 5.0);
        assert_eq!(layers[79].settings.exposure_s, 3.1);
    }

    #[test]
    fn transition_exposure_steps_down() {
        let mut p = mold_top_profile();
        p.bottom.exposure_s = 30.0;
        p.ranges[0].settings.exposure_s = 2.0;
        let layers = plan(5.75, &p).unwrap();
        let ramp: Vec<f32> = (4..11).map(|i| layers[i].settings.exposure_s).collect();
        assert_eq!(ramp, [30.0, 24.4, 18.8, 13.2, 7.6, 2.0, 2.0]);
    }
}
