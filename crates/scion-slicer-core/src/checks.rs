//! Print checks: problems in a part's layers that cause failed or messy resin prints,
//! each reported as a finding with where it is, why it matters and what to do about it.
//!
//! Works on per-layer masks from the slicing engine's rasterizer ([`rasterize_for_checks`]), in
//! print orientation: layer 0 is cured against the build plate and every later layer is
//! one step further from the plate, toward the vat film.
//!
//! - **Resin traps**: pockets enclosed in every layer's cross-section that never connect
//!   to open air on either side. Uncured resin stays sealed inside.
//! - **Suction cups**: pockets closed on the build plate side and open toward the film.
//!   The film seals the opening on every peel, so the part is pulled by a vacuum.
//! - **Peel alerts**: layers whose cured area jumps sharply, and the largest layer. Peel
//!   force grows with the area being separated from the film.
//! - **Ledges**: solid that overhangs the layer before it by more than a set distance,
//!   measured in mm from the nearest support. Short ledges print; long ones sag or fail.
//!
//! The trap and suction cup rules follow UVtools' two-pass "air map" issue detection
//! (`UVtools.Core/Managers/IssueManager.cs`, AGPL-3.0, Tiago Conceição and UVtools
//! contributors), translated to Rust; first written for DragonFruit.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use dragonfruit_islands::model::RleMask;

/// Thresholds for the checks. Areas are in mm², lengths in mm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChecksConfig {
    /// A pocket must overlap open air in the neighbouring layer by at least this much
    /// to drain into it (UVtools: 10 px).
    pub drain_overlap_mm2: f64,
    /// Smallest resin trap reported, by its largest cross-section.
    pub trap_min_area_mm2: f64,
    /// Smallest suction cup reported, by its largest cross-section (UVtools: 100 px).
    pub cup_min_area_mm2: f64,
    /// Shortest suction cup reported (UVtools: 0.5 mm).
    pub cup_min_height_mm: f64,
    /// A layer is flagged when its area grows by at least this much over the previous layer…
    pub peel_jump_min_mm2: f64,
    /// …and by at least this fraction of the previous layer's area.
    pub peel_jump_min_ratio: f64,
    /// Ledges at least this deep are reported as information.
    pub ledge_info_mm: f64,
    /// Ledges at least this deep are reported as warnings.
    pub ledge_warning_mm: f64,
}

impl Default for ChecksConfig {
    fn default() -> Self {
        Self {
            drain_overlap_mm2: 0.025,
            trap_min_area_mm2: 0.25,
            cup_min_area_mm2: 0.25,
            cup_min_height_mm: 0.5,
            peel_jump_min_mm2: 200.0,
            peel_jump_min_ratio: 0.5,
            ledge_info_mm: 1.5,
            ledge_warning_mm: 3.0,
        }
    }
}

/// Where the masks sit in the world: the grid from `rasterize_for_island_scan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksGrid {
    pub origin_x: f64,
    /// Mask row 0 is at world Y = `-origin_z` (mask Y runs along -world Y).
    pub origin_z: f64,
    pub width: i32,
    pub height: i32,
    pub px_mm: f64,
    /// World Z of the model's lowest point (the build plate side). Layer `l` is reported
    /// at its middle, `base_z + (l + 0.5) * layer_height_mm`.
    pub base_z: f64,
    pub layer_height_mm: f64,
}

impl ChecksGrid {
    fn world(&self, px: f64, py: f64, layer: usize) -> [f64; 3] {
        [
            self.origin_x + (px + 0.5) * self.px_mm,
            -(self.origin_z + (py + 0.5) * self.px_mm),
            self.base_z + (layer as f64 + 0.5) * self.layer_height_mm,
        ]
    }

    fn px_area(&self) -> f64 {
        self.px_mm * self.px_mm
    }
}

/// What a finding is. The JSON names are spelled out so the CLI's output contract can be
/// found in the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingKind {
    #[serde(rename = "resin_trap")]
    ResinTrap,
    #[serde(rename = "suction_cup")]
    SuctionCup,
    #[serde(rename = "peel_jump")]
    PeelJump,
    #[serde(rename = "largest_layer")]
    LargestLayer,
    #[serde(rename = "ledge")]
    Ledge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
}

/// One problem found in the layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    /// First and last layer (0 = against the build plate).
    pub first_layer: usize,
    pub last_layer: usize,
    /// A representative point in world mm.
    pub location_mm: [f64; 3],
    /// The size that matters for this kind: largest cross-section (mm²) for traps and
    /// cups, cured area (mm²) for peel findings, depth (mm) for ledges.
    pub size: f64,
    pub size_unit: String,
    /// Why it matters, in plain words.
    pub cause: String,
    /// What to do about it.
    pub action: String,
}

/// Rasterize a mesh into the layers the checks read, with the slicing engine's own
/// rasterizer, so the checks see each layer as it prints.
///
/// The engine samples each layer at its middle (the height a printer exposes it for) and
/// fills by non-zero winding, so holes stay open and overlapping shells merge.
/// [`rasterize_for_island_scan`] fills every slice loop on its own, which closes the holes
/// in a layer, and samples at the layer's top edge. `bbox_min` and `bbox_max` are the
/// mesh's bounds in world mm.
///
/// [`rasterize_for_island_scan`]: dragonfruit_islands::rasterize::rasterize_for_island_scan
pub fn rasterize_for_checks(
    triangles: &[dragonfruit_slicing_engine::geometry::Triangle],
    bbox_min: [f64; 3],
    bbox_max: [f64; 3],
    px_mm: f64,
    layer_height_mm: f64,
) -> (Vec<RleMask>, ChecksGrid) {
    use dragonfruit_slicing_engine::geometry::{parse_triangles, project_triangles_inplace};
    use dragonfruit_slicing_engine::index::{build_layer_index, LayerSamplingSpan};
    use dragonfruit_slicing_engine::raster::rasterize_layer;
    use dragonfruit_slicing_engine::types::SliceJobV3;

    // A grid of `px_mm` pixels around the bounding box, with a two-pixel border so no edge
    // falls on the frame. The engine maps the build area onto pixel coordinates 0..=n-1
    // with the plate centred on the origin, so the mesh is centred on the origin too and
    // the build size is (n - 1) pixels.
    const BORDER_PX: f64 = 2.0;
    let width = (((bbox_max[0] - bbox_min[0]) / px_mm).ceil() + 2.0 * BORDER_PX).max(2.0) as u32;
    let height = (((bbox_max[1] - bbox_min[1]) / px_mm).ceil() + 2.0 * BORDER_PX).max(2.0) as u32;
    let total_layers = ((bbox_max[2] - bbox_min[2]) / layer_height_mm).ceil().max(0.0) as u32;
    let (cx, cy) = ((bbox_min[0] + bbox_max[0]) / 2.0, (bbox_min[1] + bbox_max[1]) / 2.0);
    let build_width_mm = (width - 1) as f64 * px_mm;
    let build_depth_mm = (height - 1) as f64 * px_mm;
    let grid = ChecksGrid {
        origin_x: cx - build_width_mm / 2.0,
        origin_z: -(cy + build_depth_mm / 2.0),
        width: width as i32,
        height: height as i32,
        px_mm,
        base_z: bbox_min[2],
        layer_height_mm,
    };
    if total_layers == 0 {
        return (Vec::new(), grid);
    }

    let job = SliceJobV3 {
        source_width_px: width,
        source_height_px: height,
        width_px: width,
        height_px: height,
        build_width_mm: build_width_mm as f32,
        build_depth_mm: build_depth_mm as f32,
        layer_height_mm: layer_height_mm as f32,
        total_layers,
        anti_aliasing_level: "Off".to_string(),
        x_packing_mode: "none".to_string(),
        mirror_x: false,
        mirror_y: false,
        ..Default::default()
    };
    // Mesh moved so its bounding box is centred on the plate and its lowest point is z = 0.
    let mut flat = Vec::with_capacity(triangles.len() * 9);
    for t in triangles {
        for v in [t.a, t.b, t.c] {
            flat.push((v.x as f64 - cx) as f32);
            flat.push((v.y as f64 - cy) as f32);
            flat.push((v.z as f64 - bbox_min[2]) as f32);
        }
    }
    let mut projected = parse_triangles(&flat);
    project_triangles_inplace(&mut projected, &job);
    let index = build_layer_index(
        &projected,
        total_layers,
        job.layer_height_mm,
        LayerSamplingSpan::CenterOnly,
    );

    let masks = (0..total_layers)
        .into_par_iter()
        .map(|l| {
            let dense = rasterize_layer(&job, &projected, index.candidates_for_layer(l), l);
            dragonfruit_islands::rle::rle_encode(&dense, width as i32, height as i32)
        })
        .collect();
    (masks, grid)
}

/// Run every check over the layers. Findings are sorted warnings first, then by layer.
pub fn run_checks(masks: &[RleMask], grid: &ChecksGrid, cfg: &ChecksConfig) -> Vec<Finding> {
    let mut findings = trap_and_cup_findings(masks, grid, cfg);
    findings.extend(peel_findings(masks, grid, cfg));
    findings.extend(ledge_findings(masks, grid, cfg));
    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.first_layer.cmp(&b.first_layer)));
    findings
}

// ---------------------------------------------------------------------------
// Bitmaps
// ---------------------------------------------------------------------------

fn decode(mask: &RleMask) -> Vec<bool> {
    let (w, h) = (mask.width as usize, mask.height as usize);
    let mut out = vec![false; w * h];
    for (y, row) in mask.rows.iter().enumerate().take(h) {
        for run in row {
            let a = (run.start.max(0) as usize).min(w);
            let b = ((run.start + run.length).max(0) as usize).min(w);
            out[y * w + a..y * w + b].fill(true);
        }
    }
    out
}

/// A pocket in one layer: background enclosed by solid, as row runs `(row, start, end)`.
#[derive(Debug, Clone)]
struct Pocket {
    runs: Vec<(u32, u32, u32)>,
    area_px: u64,
    sum_x: f64,
    sum_y: f64,
}

impl Pocket {
    fn overlap(&self, map: &[bool], w: usize) -> u64 {
        self.runs
            .iter()
            .map(|&(y, a, b)| {
                let row = y as usize * w;
                map[row + a as usize..row + b as usize].iter().filter(|&&v| v).count() as u64
            })
            .sum()
    }

    fn paint(&self, map: &mut [bool], w: usize) {
        for &(y, a, b) in &self.runs {
            let row = y as usize * w;
            map[row + a as usize..row + b as usize].fill(true);
        }
    }

    fn paint_id(&self, map: &mut [u32], w: usize, id: u32) {
        for &(y, a, b) in &self.runs {
            let row = y as usize * w;
            map[row + a as usize..row + b as usize].fill(id);
        }
    }
}

/// Background connected to the grid edge (4-connected) is outside; every other
/// background region is a pocket. Returns the outside map and the pockets.
fn outside_and_pockets(solid: &[bool], w: usize, h: usize) -> (Vec<bool>, Vec<Pocket>) {
    let mut outside = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let seed = |i: usize, outside: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if !solid[i] && !outside[i] {
            outside[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        seed(x, &mut outside, &mut stack);
        seed((h - 1) * w + x, &mut outside, &mut stack);
    }
    for y in 0..h {
        seed(y * w, &mut outside, &mut stack);
        seed(y * w + w - 1, &mut outside, &mut stack);
    }
    flood4(solid, &mut outside, &mut stack, w, h);

    let mut seen = outside.clone();
    let mut pockets = Vec::new();
    for start in 0..w * h {
        if solid[start] || seen[start] {
            continue;
        }
        let mut pixels = Vec::new();
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            pixels.push(i);
            let (x, y) = (i % w, i / w);
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < w && ny < h {
                    let j = ny * w + nx;
                    if !solid[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        pixels.sort_unstable();
        let mut runs: Vec<(u32, u32, u32)> = Vec::new();
        let (mut sum_x, mut sum_y) = (0.0, 0.0);
        for &i in &pixels {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            sum_x += x as f64;
            sum_y += y as f64;
            match runs.last_mut() {
                Some(last) if last.0 == y && last.2 == x => last.2 += 1,
                _ => runs.push((y, x, x + 1)),
            }
        }
        pockets.push(Pocket { runs, area_px: pixels.len() as u64, sum_x, sum_y });
    }
    (outside, pockets)
}

fn flood4(solid: &[bool], filled: &mut [bool], stack: &mut Vec<usize>, w: usize, h: usize) {
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx < w && ny < h {
                let j = ny * w + nx;
                if !solid[j] && !filled[j] {
                    filled[j] = true;
                    stack.push(j);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Resin traps and suction cups
// ---------------------------------------------------------------------------

fn trap_and_cup_findings(masks: &[RleMask], grid: &ChecksGrid, cfg: &ChecksConfig) -> Vec<Finding> {
    let n = masks.len();
    if n == 0 {
        return Vec::new();
    }
    let (w, h) = (grid.width as usize, grid.height as usize);
    let drain_px = (cfg.drain_overlap_mm2 / grid.px_area()).ceil().max(1.0) as u64;

    // Outside air and pockets of every layer (independent per layer).
    let layers: Vec<(Vec<bool>, Vec<Pocket>)> = masks
        .par_iter()
        .map(|m| outside_and_pockets(&decode(m), w, h))
        .collect();

    // Pass 1, from the build plate outward: a pocket drains when it overlaps air in the
    // layer before it. Before layer 0 is the build plate itself; a pocket against the
    // plate is not a trap (it opens as soon as the part lifts), as in UVtools.
    let mut drained_plate_side: Vec<Vec<bool>> = Vec::with_capacity(n);
    let mut air_prev = vec![true; w * h];
    for (outside, pockets) in &layers {
        let drained: Vec<bool> = pockets.iter().map(|p| p.overlap(&air_prev, w) >= drain_px).collect();
        let mut air = outside.clone();
        for (p, &d) in pockets.iter().zip(&drained) {
            if d {
                p.paint(&mut air, w);
            }
        }
        drained_plate_side.push(drained);
        air_prev = air;
    }

    // Pass 2, from the last layer back: a pocket is open toward the film when it overlaps
    // air in the layer after it. After the last layer is open air.
    let mut open_film_side: Vec<Vec<bool>> = vec![Vec::new(); n];
    let mut air_next = vec![true; w * h];
    for l in (0..n).rev() {
        let (outside, pockets) = &layers[l];
        let open: Vec<bool> = pockets.iter().map(|p| p.overlap(&air_next, w) >= drain_px).collect();
        let mut air = outside.clone();
        for (pi, (p, &o)) in pockets.iter().zip(&open).enumerate() {
            if o || drained_plate_side[l][pi] {
                p.paint(&mut air, w);
            }
        }
        open_film_side[l] = open;
        air_next = air;
    }

    // Group the undrained pockets through the layers: pockets in neighbouring layers that
    // overlap belong to the same cavity.
    let mut parent: Vec<usize> = Vec::new();
    let mut node_of: Vec<Vec<Option<usize>>> = Vec::with_capacity(n);
    let mut nodes: Vec<(usize, usize)> = Vec::new(); // (layer, pocket index)
    let mut prev_ids = vec![0u32; w * h];
    let mut prev_present = false;
    for l in 0..n {
        let pockets = &layers[l].1;
        let mut ids = vec![0u32; w * h];
        let mut this_layer = vec![None; pockets.len()];
        for (pi, p) in pockets.iter().enumerate() {
            if drained_plate_side[l][pi] {
                continue;
            }
            let node = nodes.len();
            nodes.push((l, pi));
            parent.push(node);
            this_layer[pi] = Some(node);
            p.paint_id(&mut ids, w, node as u32 + 1);
            if prev_present {
                for &(y, a, b) in &p.runs {
                    let row = y as usize * w;
                    for &id in &prev_ids[row + a as usize..row + b as usize] {
                        if id != 0 {
                            union(&mut parent, node, id as usize - 1);
                        }
                    }
                }
            }
        }
        prev_present = this_layer.iter().any(Option::is_some);
        node_of.push(this_layer);
        prev_ids = ids;
    }

    // Summarize each cavity.
    struct Cavity {
        first: usize,
        last: usize,
        open_to_film: bool,
        max_area_px: u64,
        at: (f64, f64, usize),
        volume_px: u64,
    }
    let mut cavities: std::collections::BTreeMap<usize, Cavity> = Default::default();
    for (node, &(l, pi)) in nodes.iter().enumerate() {
        let root = find(&mut parent, node);
        let p = &layers[l].1[pi];
        let c = cavities.entry(root).or_insert(Cavity {
            first: l,
            last: l,
            open_to_film: false,
            max_area_px: 0,
            at: (0.0, 0.0, l),
            volume_px: 0,
        });
        c.first = c.first.min(l);
        c.last = c.last.max(l);
        c.open_to_film |= open_film_side[l][pi];
        c.volume_px += p.area_px;
        if p.area_px > c.max_area_px {
            c.max_area_px = p.area_px;
            c.at = (p.sum_x / p.area_px as f64, p.sum_y / p.area_px as f64, l);
        }
    }

    let mut out = Vec::new();
    for c in cavities.values() {
        let area = c.max_area_px as f64 * grid.px_area();
        let height = (c.last - c.first + 1) as f64 * grid.layer_height_mm;
        let volume = c.volume_px as f64 * grid.px_area() * grid.layer_height_mm;
        let location_mm = grid.world(c.at.0, c.at.1, c.at.2);
        if c.open_to_film {
            if area < cfg.cup_min_area_mm2 || height < cfg.cup_min_height_mm {
                continue;
            }
            out.push(Finding {
                kind: FindingKind::SuctionCup,
                severity: Severity::Warning,
                first_layer: c.first,
                last_layer: c.last,
                location_mm,
                size: area,
                size_unit: "mm2".into(),
                cause: format!(
                    "A {area:.1} mm² cavity, {height:.1} mm deep, is closed on the build plate side and open \
                     toward the film. The film seals it on every peel, so each lift pulls against a vacuum: \
                     layer shifts, failed prints or a torn film."
                ),
                action: "Turn the part so the cavity opens toward the build plate, or add a vent hole through \
                         its closed end."
                    .into(),
            });
        } else {
            if area < cfg.trap_min_area_mm2 {
                continue;
            }
            out.push(Finding {
                kind: FindingKind::ResinTrap,
                severity: Severity::Warning,
                first_layer: c.first,
                last_layer: c.last,
                location_mm,
                size: area,
                size_unit: "mm2".into(),
                cause: format!(
                    "A sealed cavity ({volume:.1} mm³, {height:.1} mm tall, up to {area:.1} mm² across) has no \
                     opening on either side. The uncured resin inside stays there: it can crack the part \
                     later, or leak."
                ),
                action: "Add a drain hole at the cavity's lowest point and a vent at its highest, or fill the \
                         cavity."
                    .into(),
            });
        }
    }
    out
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

// ---------------------------------------------------------------------------
// Peel alerts
// ---------------------------------------------------------------------------

fn peel_findings(masks: &[RleMask], grid: &ChecksGrid, cfg: &ChecksConfig) -> Vec<Finding> {
    let areas: Vec<f64> = masks.iter().map(|m| m.pixel_count() as f64 * grid.px_area()).collect();
    let centre = |m: &RleMask| -> (f64, f64) {
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        for (y, row) in m.rows.iter().enumerate() {
            for r in row {
                let len = r.length as f64;
                sx += (r.start as f64 + (len - 1.0) / 2.0) * len;
                sy += y as f64 * len;
                n += len;
            }
        }
        if n > 0.0 { (sx / n, sy / n) } else { (0.0, 0.0) }
    };
    let mut out = Vec::new();
    if let Some((l, &area)) = areas.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)) {
        if area > 0.0 {
            let (cx, cy) = centre(&masks[l]);
            out.push(Finding {
                kind: FindingKind::LargestLayer,
                severity: Severity::Info,
                first_layer: l,
                last_layer: l,
                location_mm: grid.world(cx, cy, l),
                size: area,
                size_unit: "mm2".into(),
                cause: format!(
                    "Layer {} is the largest, {area:.0} mm². Peel force grows with the area pulled off the film.",
                    l + 1
                ),
                action: "If prints fail around this height, lift more slowly on the largest layers, or tilt \
                         the part so less area separates at once."
                    .into(),
            });
        }
    }
    for l in 1..areas.len() {
        let (before, now) = (areas[l - 1], areas[l]);
        let jump = now - before;
        if jump >= cfg.peel_jump_min_mm2 && jump >= cfg.peel_jump_min_ratio * before {
            let (cx, cy) = centre(&masks[l]);
            out.push(Finding {
                kind: FindingKind::PeelJump,
                severity: Severity::Warning,
                first_layer: l,
                last_layer: l,
                location_mm: grid.world(cx, cy, l),
                size: now,
                size_unit: "mm2".into(),
                cause: format!(
                    "The cured area jumps from {before:.0} to {now:.0} mm² at layer {}. A sudden jump in area \
                     is a sudden jump in peel force, a common place for layers to separate.",
                    l + 1
                ),
                action: "Tilt the part so its cross-section grows gradually, or slow the lift for the layers \
                         around this one."
                    .into(),
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Ledges
// ---------------------------------------------------------------------------

fn ledge_findings(masks: &[RleMask], grid: &ChecksGrid, cfg: &ChecksConfig) -> Vec<Finding> {
    let (w, h) = (grid.width as usize, grid.height as usize);
    let info_px = cfg.ledge_info_mm / grid.px_mm;
    let mut per_layer: Vec<Vec<Finding>> = (1..masks.len())
        .into_par_iter()
        .map(|l| {
            let below = decode(&masks[l - 1]);
            if !below.iter().any(|&v| v) {
                return Vec::new();
            }
            let here = decode(&masks[l]);
            let overhang: Vec<bool> = here.iter().zip(&below).map(|(&a, &b)| a && !b).collect();
            if !overhang.iter().any(|&v| v) {
                return Vec::new();
            }
            let dist2 = squared_distance_to(&below, w, h);
            let mut out = Vec::new();
            for comp in components8(&overhang, w, h) {
                // Deepest point of this overhanging region, in pixels from the nearest
                // solid of the layer before. Regions touching nothing below are islands,
                // which the island scan reports.
                let (mut deepest, mut at) = (0.0f32, comp[0]);
                let mut touches_support = false;
                for &i in &comp {
                    let d = dist2[i];
                    if d <= 2.0 {
                        touches_support = true;
                    }
                    if d > deepest {
                        deepest = d;
                        at = i;
                    }
                }
                let depth_px = (deepest as f64).sqrt();
                if !touches_support || depth_px < info_px {
                    continue;
                }
                let depth = depth_px * grid.px_mm;
                let severity = if depth >= cfg.ledge_warning_mm { Severity::Warning } else { Severity::Info };
                out.push(Finding {
                    kind: FindingKind::Ledge,
                    severity,
                    first_layer: l,
                    last_layer: l,
                    location_mm: grid.world((at % w) as f64, (at / w) as f64, l),
                    size: depth,
                    size_unit: "mm".into(),
                    cause: format!(
                        "Layer {} overhangs the layer before it by up to {depth:.2} mm with nothing underneath. \
                         The unsupported edge is cured hanging in resin; long ledges sag, curl or tear off on \
                         the peel.",
                        l + 1
                    ),
                    action: format!(
                        "Below about {:.1} mm a ledge usually prints. Deeper: add a 45° chamfer or fillet \
                         under it in the model, turn the part, or support the edge.",
                        cfg.ledge_warning_mm
                    ),
                });
            }
            out
        })
        .collect();
    per_layer.iter_mut().flat_map(std::mem::take).collect()
}

/// 8-connected components of `set`, as pixel index lists.
fn components8(set: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !set[start] || seen[start] {
            continue;
        }
        let mut comp = Vec::new();
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i as isize % w as isize, i as isize / w as isize);
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let j = ny as usize * w + nx as usize;
                        if set[j] && !seen[j] {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        out.push(comp);
    }
    out
}

/// Squared distance in pixels from every pixel to the nearest pixel of `set`
/// (exact Euclidean distance transform, Felzenszwalb & Huttenlocher).
fn squared_distance_to(set: &[bool], w: usize, h: usize) -> Vec<f32> {
    let inf = f32::INFINITY;
    let mut out = vec![inf; w * h];
    let n = w.max(h);
    let (mut f, mut d, mut v, mut z) = (vec![0f32; n], vec![0f32; n], vec![0usize; n], vec![0f32; n + 1]);
    for y in 0..h {
        for x in 0..w {
            f[x] = if set[y * w + x] { 0.0 } else { inf };
        }
        distance_1d(&f[..w], &mut d[..w], &mut v, &mut z);
        out[y * w..(y + 1) * w].copy_from_slice(&d[..w]);
    }
    for x in 0..w {
        for y in 0..h {
            f[y] = out[y * w + x];
        }
        distance_1d(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            out[y * w + x] = d[y];
        }
    }
    out
}

fn distance_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    let Some(first) = f.iter().position(|x| x.is_finite()) else {
        d.fill(f32::INFINITY);
        return;
    };
    let mut k = 0usize;
    v[0] = first;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in first + 1..n {
        if !f[q].is_finite() {
            continue;
        }
        loop {
            let p = v[k];
            let (qf, pf) = (q as f32, p as f32);
            let s = ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * (qf - pf));
            if s <= z[k] {
                if k == 0 {
                    v[0] = q;
                    z[1] = f32::INFINITY;
                    break;
                }
                k -= 1;
                continue;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f32::INFINITY;
            break;
        }
    }
    k = 0;
    for (q, dq_out) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let dq = q as f32 - v[k] as f32;
        *dq_out = dq * dq + f[v[k]];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dragonfruit_islands::model::RleRun;

    const W: i32 = 120;
    const H: i32 = 120;

    /// A mask from rectangles `(x0, y0, x1, y1, solid)`, applied in order.
    fn mask(rects: &[(i32, i32, i32, i32, bool)]) -> RleMask {
        let mut px = vec![false; (W * H) as usize];
        for &(x0, y0, x1, y1, v) in rects {
            for y in y0..y1 {
                for x in x0..x1 {
                    px[(y * W + x) as usize] = v;
                }
            }
        }
        let mut m = RleMask::empty(W, H);
        for y in 0..H as usize {
            let row = &px[y * W as usize..(y + 1) * W as usize];
            let mut x = 0;
            while x < row.len() {
                if row[x] {
                    let s = x;
                    while x < row.len() && row[x] {
                        x += 1;
                    }
                    m.rows[y].push(RleRun { start: s as i32, length: (x - s) as i32 });
                } else {
                    x += 1;
                }
            }
        }
        m
    }

    fn grid() -> ChecksGrid {
        // 0.1 mm pixels and layers
        ChecksGrid { origin_x: 0.0, origin_z: -12.0, width: W, height: H, px_mm: 0.1, base_z: 0.0, layer_height_mm: 0.1 }
    }

    const BLOCK: (i32, i32, i32, i32, bool) = (20, 20, 100, 100, true);
    const HOLE: (i32, i32, i32, i32, bool) = (50, 50, 70, 70, false);

    fn kinds(f: &[Finding]) -> Vec<FindingKind> {
        f.iter().map(|f| f.kind).filter(|k| *k != FindingKind::LargestLayer).collect()
    }

    #[test]
    fn closed_hollow_is_a_resin_trap() {
        // solid floor, 10 layers with a pocket, solid lid
        let mut layers = vec![mask(&[BLOCK]); 3];
        layers.extend(vec![mask(&[BLOCK, HOLE]); 10]);
        layers.extend(vec![mask(&[BLOCK]); 3]);
        let f = run_checks(&layers, &grid(), &ChecksConfig::default());
        assert_eq!(kinds(&f), vec![FindingKind::ResinTrap]);
        let trap = &f[0];
        assert_eq!((trap.first_layer, trap.last_layer), (3, 12));
        assert!((trap.size - 4.0).abs() < 1e-9, "2 x 2 mm pocket: {}", trap.size);
    }

    #[test]
    fn cup_open_toward_the_film_is_a_suction_cup() {
        // closed on the plate side, open after the last layer
        let mut layers = vec![mask(&[BLOCK]); 3];
        layers.extend(vec![mask(&[BLOCK, HOLE]); 10]);
        let f = run_checks(&layers, &grid(), &ChecksConfig::default());
        assert_eq!(kinds(&f), vec![FindingKind::SuctionCup]);
    }

    #[test]
    fn cup_open_toward_the_plate_drains() {
        // the same cup the other way up: open on the plate side, closed later
        let mut layers = vec![mask(&[BLOCK, HOLE]); 10];
        layers.extend(vec![mask(&[BLOCK]); 3]);
        let f = run_checks(&layers, &grid(), &ChecksConfig::default());
        assert!(kinds(&f).is_empty(), "{:?}", kinds(&f));
    }

    #[test]
    fn short_cups_are_ignored() {
        let mut layers = vec![mask(&[BLOCK]); 3];
        layers.extend(vec![mask(&[BLOCK, HOLE]); 4]); // 0.4 mm < 0.5 mm
        let f = run_checks(&layers, &grid(), &ChecksConfig::default());
        assert!(kinds(&f).is_empty(), "{:?}", kinds(&f));
    }

    #[test]
    fn sudden_area_jump_is_a_peel_alert() {
        let small = mask(&[(50, 50, 70, 70, true)]); // 4 mm²
        let big = mask(&[(10, 10, 110, 110, true)]); // 100 mm²
        let cfg = ChecksConfig { peel_jump_min_mm2: 50.0, ..Default::default() };
        let mut layers = vec![small; 5];
        layers.extend(vec![big; 5]);
        let f = run_checks(&layers, &grid(), &cfg);
        let jumps: Vec<_> = f.iter().filter(|f| f.kind == FindingKind::PeelJump).collect();
        assert_eq!(jumps.len(), 1);
        assert_eq!(jumps[0].first_layer, 5);
        assert!(f.iter().any(|f| f.kind == FindingKind::LargestLayer));
    }

    #[test]
    fn ledge_depth_is_measured_in_mm() {
        // a 40 mm... at 0.1 mm px: shelf sticking out 5 mm (50 px) and 2 mm (20 px)
        let base = mask(&[(20, 20, 60, 100, true)]);
        let deep = mask(&[(20, 20, 110, 100, true)]); // +50 px on the right
        let shallow = mask(&[(20, 20, 80, 100, true)]); // +20 px
        let f = run_checks(&[base.clone(), deep], &grid(), &ChecksConfig::default());
        let ledges: Vec<_> = f.iter().filter(|f| f.kind == FindingKind::Ledge).collect();
        assert_eq!(ledges.len(), 1);
        assert!((ledges[0].size - 5.0).abs() < 0.11, "depth {}", ledges[0].size);
        assert_eq!(ledges[0].severity, Severity::Warning);

        let f = run_checks(&[base, shallow], &grid(), &ChecksConfig::default());
        let ledges: Vec<_> = f.iter().filter(|f| f.kind == FindingKind::Ledge).collect();
        assert_eq!(ledges.len(), 1);
        assert!((ledges[0].size - 2.0).abs() < 0.11, "depth {}", ledges[0].size);
        assert_eq!(ledges[0].severity, Severity::Info);
    }

    #[test]
    fn floating_regions_are_left_to_the_island_scan() {
        let a = mask(&[(20, 20, 40, 40, true)]);
        let b = mask(&[(20, 20, 40, 40, true), (80, 80, 110, 110, true)]);
        let f = run_checks(&[a, b], &grid(), &ChecksConfig::default());
        assert!(f.iter().all(|f| f.kind != FindingKind::Ledge));
    }

    /// A 10 x 10 x 1 mm plate with a 4 x 4 mm square through-hole, as flat triangle
    /// positions (outward-facing).
    fn plate_with_hole() -> Vec<dragonfruit_slicing_engine::geometry::Triangle> {
        let mut flat: Vec<f32> = Vec::new();
        let mut quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| {
            for t in [[a, b, c], [a, c, d]] {
                for v in t {
                    flat.extend_from_slice(&v);
                }
            }
        };
        let (o, i, t) = ([0.0f32, 0.0, 10.0, 10.0], [3.0f32, 3.0, 7.0, 7.0], 1.0f32);
        let ring = |r: [f32; 4]| [(r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3])];
        let (oc, ic) = (ring(o), ring(i));
        for k in 0..4 {
            let (a, b) = (oc[k], oc[(k + 1) % 4]);
            let (c, d) = (ic[(k + 1) % 4], ic[k]);
            quad([a.0, a.1, t], [b.0, b.1, t], [c.0, c.1, t], [d.0, d.1, t]); // top
            quad([a.0, a.1, 0.0], [d.0, d.1, 0.0], [c.0, c.1, 0.0], [b.0, b.1, 0.0]); // bottom
            quad([a.0, a.1, 0.0], [b.0, b.1, 0.0], [b.0, b.1, t], [a.0, a.1, t]); // outer wall
            quad([d.0, d.1, 0.0], [d.0, d.1, t], [c.0, c.1, t], [c.0, c.1, 0.0]); // hole wall
        }
        dragonfruit_slicing_engine::geometry::parse_triangles(&flat)
    }

    #[test]
    fn every_layer_keeps_the_hole_including_the_top_one() {
        let tris = plate_with_hole();
        let (masks, grid) = rasterize_for_checks(&tris, [0.0, 0.0, 0.0], [10.0, 10.0, 1.0], 0.05, 0.05);
        let areas: Vec<f64> = masks.iter().map(|m| m.pixel_count() as f64 * grid.px_area()).collect();
        eprintln!("{} layers: {:?}", areas.len(), areas.iter().map(|a| (a * 100.0).round() / 100.0).collect::<Vec<_>>());
        assert_eq!(areas.len(), 20);
        for (l, a) in areas.iter().enumerate() {
            assert!((a - 84.0).abs() < 0.5, "layer {l}: {a} mm2, expected 84");
        }
    }

    /// Triangle positions of an axis-aligned box, faces outward (or inward, for a cavity).
    fn push_box(flat: &mut Vec<f32>, lo: [f32; 3], hi: [f32; 3], inward: bool) {
        let c = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
        // Each face as four corners, counter-clockwise seen from outside.
        for f in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            let mut q = f.map(c);
            if inward {
                q.reverse();
            }
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                for v in t {
                    flat.extend_from_slice(&v);
                }
            }
        }
    }

    #[test]
    fn sealed_cavity_in_a_mesh_is_a_resin_trap() {
        let mut flat = Vec::new();
        push_box(&mut flat, [0.0, 0.0, 0.0], [10.0, 10.0, 6.0], false);
        push_box(&mut flat, [3.0, 3.0, 2.0], [7.0, 7.0, 4.0], true);
        let tris = dragonfruit_slicing_engine::geometry::parse_triangles(&flat);
        let (masks, grid) = rasterize_for_checks(&tris, [0.0, 0.0, 0.0], [10.0, 10.0, 6.0], 0.05, 0.05);
        let f = run_checks(&masks, &grid, &ChecksConfig::default());
        let traps: Vec<_> = f.iter().filter(|f| f.kind == FindingKind::ResinTrap).collect();
        assert_eq!(traps.len(), 1, "{f:?}");
        assert!((traps[0].size - 16.0).abs() < 0.5, "trap cross-section {} mm2", traps[0].size);
        let [x, y, _] = traps[0].location_mm;
        assert!((3.0..=7.0).contains(&x) && (3.0..=7.0).contains(&y), "trap at {x}, {y}");
    }
}
