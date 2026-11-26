//! Writing a `.goo` file: the header, one record per layer, and the footer.

use std::io::{self, Seek, SeekFrom, Write};

use crate::rle::encode_layer;
use crate::settings::{round_to, Machine, Resin};

/// Size of the header; the first layer record starts here.
pub const HEADER_SIZE: usize = 195_477;
/// Where the layer count sits, right after the two previews.
pub const PARAMS_OFFSET: usize = 195_310;
/// The version SatelLite 1.0.2.43 writes and opens.
pub const VERSION: &[u8; 4] = b"V3.0";
pub const MAGIC: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x44, 0x4C, 0x50, 0x00];
pub const FOOTER: [u8; 11] = [
    0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x44, 0x4C, 0x50, 0x00,
];
pub const SMALL_PREVIEW: usize = 116;
pub const BIG_PREVIEW: usize = 290;
const CRLF: [u8; 2] = [0x0D, 0x0A];
/// SatelLite writes this in every lift and retract field: the tilting vat ignores them.
const PLACEHOLDER: f32 = 0.05;

/// Who wrote the file, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub software: String,
    pub version: String,
    /// "YYYY-MM-DD HH:MM:SS".
    pub created: String,
}

impl FileInfo {
    /// Scion Slicer, this crate's version, now (UTC).
    pub fn scion_slicer() -> Self {
        Self {
            software: "Scion Slicer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            created: utc_now(),
        }
    }
}

/// A layer encoded ahead of time, so layers can be encoded in parallel and written in order.
#[derive(Debug, Clone)]
pub struct PreparedLayer {
    pub data: Vec<u8>,
    /// Sum of the pixel values / 255: the number of fully lit pixels it equals.
    pub lit_pixels: f64,
    footprint: Footprint,
}

/// Encode one layer image for `machine`: one byte per pixel, row by row, already
/// mirrored as the printer needs.
pub fn prepare_layer(machine: &Machine, pixels: &[u8]) -> Result<PreparedLayer, String> {
    if pixels.len() != machine.pixel_count() {
        return Err(format!(
            "layer has {} pixels, the {} has {}",
            pixels.len(),
            machine.name,
            machine.pixel_count()
        ));
    }
    let lit: u64 = pixels.iter().map(|&p| p as u64).sum();
    Ok(PreparedLayer {
        data: encode_layer(pixels),
        lit_pixels: lit as f64 / 255.0,
        footprint: Footprint::sample(machine, pixels),
    })
}

/// What the finished file says about the print.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub layers: u32,
    pub print_time_s: u32,
    pub volume_mm3: f32,
    pub weight_g: f32,
    pub cost: f32,
}

/// Writes a `.goo` file layer by layer, then goes back and fills in the header.
pub struct GooWriter<W: Write + Seek> {
    out: W,
    machine: Machine,
    resin: Resin,
    info: FileInfo,
    layers: u32,
    /// SatelLite adds the time up in 32-bit floats; so do we, to round down the same way.
    time_s: f32,
    lit_pixels: f64,
    footprint: Footprint,
}

impl<W: Write + Seek> GooWriter<W> {
    pub fn new(mut out: W, machine: Machine, resin: Resin, info: FileInfo) -> io::Result<Self> {
        out.write_all(&vec![0u8; HEADER_SIZE])?;
        let footprint = Footprint::empty();
        Ok(Self {
            out,
            machine,
            resin,
            info,
            layers: 0,
            time_s: 0.0,
            lit_pixels: 0.0,
            footprint,
        })
    }

    /// Add the next layer, counting from the plate.
    pub fn add_layer(&mut self, pixels: &[u8]) -> io::Result<()> {
        let layer = prepare_layer(&self.machine, pixels).map_err(io::Error::other)?;
        self.add_prepared(&layer)
    }

    /// Add the next layer, encoded with [`prepare_layer`].
    pub fn add_prepared(&mut self, layer: &PreparedLayer) -> io::Result<()> {
        let index = self.layers;
        self.out
            .write_all(&layer_record(&self.machine, &self.resin, index))?;
        self.out
            .write_all(&(layer.data.len() as u32).to_be_bytes())?;
        self.out.write_all(&layer.data)?;
        self.out.write_all(&CRLF)?;

        let settings = self.resin.settings_for(index);
        let w = settings.waits;
        let step = self.resin.exposure_s(index) as f64
            + w.before_cure_s as f64
            + w.before_release_s as f64
            + w.after_release_s as f64
            + self.machine.tilt_seconds_per_layer
            + settings.motion.seconds();
        self.time_s += step as f32;
        self.lit_pixels += layer.lit_pixels;
        self.footprint.add(&layer.footprint);
        self.layers += 1;
        Ok(())
    }

    /// Write the footer and the header. `volume_mm3` is the mesh volume when the caller
    /// knows it, as SatelLite uses; otherwise it's counted from the lit pixels.
    pub fn finish(mut self, volume_mm3: Option<f64>) -> io::Result<(W, Summary)> {
        self.out.write_all(&FOOTER)?;
        let volume = volume_mm3.unwrap_or_else(|| {
            self.lit_pixels * self.machine.pixel_area_mm2() * self.resin.layer_height_mm as f64
        }) as f32;
        // Each figure is rounded to thousandths and stored as a 32-bit float before the
        // next is worked out from it: 31.05 g is 31.0499992 as a float, which costs
        // $0.931 at $30/kg, not $0.932.
        let weight = round_to(volume as f64 * self.resin.density_g_ml as f64 / 1000.0, 3) as f32;
        let cost = round_to(weight as f64 * self.resin.price_per_kg as f64 / 1000.0, 3);
        let summary = Summary {
            layers: self.layers,
            print_time_s: self.time_s.floor() as u32,
            volume_mm3: volume,
            weight_g: weight,
            cost: cost as f32,
        };
        let header = header(
            &self.machine,
            &self.resin,
            &self.info,
            &summary,
            &self.footprint,
        );
        self.out.seek(SeekFrom::Start(0))?;
        self.out.write_all(&header)?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()?;
        Ok((self.out, summary))
    }
}

fn header(
    machine: &Machine,
    resin: &Resin,
    info: &FileInfo,
    s: &Summary,
    footprint: &Footprint,
) -> Vec<u8> {
    let mut h = Bytes(Vec::with_capacity(HEADER_SIZE));
    h.raw(VERSION);
    h.raw(&MAGIC);
    h.text(&info.software, 32);
    h.text(&info.version, 24);
    h.text(&info.created, 24);
    h.text(&machine.name, 32); // printer name
    h.text(&machine.name, 32); // printer type
    h.text(&resin.profile_name, 32);
    h.u16(0); // anti-aliasing level
    h.u16(0); // grey level
    h.u16(0); // blur level
    h.raw(&footprint.preview(machine, SMALL_PREVIEW));
    h.raw(&CRLF);
    h.raw(&footprint.preview(machine, BIG_PREVIEW));
    h.raw(&CRLF);
    debug_assert_eq!(h.0.len(), PARAMS_OFFSET);

    let (b, n) = (&resin.bottom, &resin.normal);
    h.u32(s.layers);
    h.u16(machine.resolution_x);
    h.u16(machine.resolution_y);
    h.u8(machine.mirror_x as u8);
    h.u8(machine.mirror_y as u8);
    h.f32(machine.display_width_mm);
    h.f32(machine.display_height_mm);
    h.f32(machine.machine_z_mm);
    h.f32(resin.layer_height_mm);
    h.f32(n.exposure_s);
    h.u8(1); // delay mode: static waits
    h.f32(0.0); // light-off time
    for waits in [b.waits, n.waits] {
        h.f32(waits.before_release_s); // spec: before lift
        h.f32(waits.after_release_s); // spec: after lift
        h.f32(waits.before_cure_s); // spec: after retract
    }
    h.f32(b.exposure_s);
    h.u32(resin.bottom_layers);
    for _ in 0..8 {
        h.f32(PLACEHOLDER); // lift and retract, bottom and normal, distance and speed
    }
    for _ in 0..8 {
        h.f32(0.0); // the same for the second stage
    }
    h.u16(b.pwm);
    h.u16(n.pwm);
    h.u8(0); // advance mode off: the printer uses these header settings
    h.u32(s.print_time_s);
    h.f32(s.volume_mm3);
    h.f32(s.weight_g);
    h.f32(s.cost);
    h.text(&resin.currency, 8);
    h.u32(HEADER_SIZE as u32);
    h.u8(0); // gray scale level
    h.u16(resin.transition_layers);
    debug_assert_eq!(h.0.len(), HEADER_SIZE);
    h.0
}

fn layer_record(machine: &Machine, resin: &Resin, index: u32) -> Vec<u8> {
    let settings = resin.settings_for(index);
    let mut r = Bytes(Vec::with_capacity(70));
    r.u16(0); // pause flag
    r.f32(machine.pause_position_z_mm);
    r.f32(resin.layer_z_mm(index));
    r.f32(resin.exposure_s(index));
    r.f32(0.0); // light-off time
    r.f32(settings.waits.before_release_s);
    r.f32(settings.waits.after_release_s);
    r.f32(settings.waits.before_cure_s);
    for _ in 0..8 {
        r.f32(PLACEHOLDER); // lift, second lift, retract, second retract: distance and speed
    }
    r.u16(settings.pwm);
    r.raw(&CRLF);
    r.0
}

struct Bytes(Vec<u8>);

impl Bytes {
    fn raw(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.raw(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.raw(&v.to_be_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.raw(&v.to_be_bytes());
    }
    /// A fixed-size string, zero-padded, always ending in at least one zero.
    fn text(&mut self, s: &str, size: usize) {
        let bytes = s.as_bytes();
        let n = bytes.len().min(size - 1);
        self.raw(&bytes[..n]);
        self.0.resize(self.0.len() + size - n, 0);
    }
}

/// A coarse top view of where the print has lit pixels, for the preview images.
#[derive(Debug, Clone)]
struct Footprint(Vec<bool>);

/// Samples across and down the screen.
const FOOT_W: usize = BIG_PREVIEW;
const FOOT_H: usize = BIG_PREVIEW;

impl Footprint {
    fn empty() -> Self {
        Self(vec![false; FOOT_W * FOOT_H])
    }

    fn sample(machine: &Machine, pixels: &[u8]) -> Self {
        let (w, h) = (machine.resolution_x as usize, machine.resolution_y as usize);
        let mut cells = vec![false; FOOT_W * FOOT_H];
        for cy in 0..FOOT_H {
            let y = (cy * 2 + 1) * h / (FOOT_H * 2);
            let row = &pixels[y * w..(y + 1) * w];
            for cx in 0..FOOT_W {
                cells[cy * FOOT_W + cx] = row[(cx * 2 + 1) * w / (FOOT_W * 2)] != 0;
            }
        }
        Self(cells)
    }

    fn add(&mut self, other: &Footprint) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= *b;
        }
    }

    /// A square RGB565 (big-endian) image: the screen fitted inside, lit areas in blue.
    fn preview(&self, machine: &Machine, size: usize) -> Vec<u8> {
        const BACKGROUND: u16 = rgb565(0x24, 0x24, 0x28);
        const SCREEN: u16 = rgb565(0x10, 0x10, 0x12);
        const PRINT: u16 = rgb565(0x7D, 0x8B, 0xF0);
        let aspect = machine.display_height_mm as f64 / machine.display_width_mm as f64;
        let (sw, sh) = if aspect <= 1.0 {
            (size, ((size as f64 * aspect).round() as usize).max(1))
        } else {
            (((size as f64 / aspect).round() as usize).max(1), size)
        };
        let (x0, y0) = ((size - sw) / 2, (size - sh) / 2);
        let mut out = Vec::with_capacity(size * size * 2);
        for y in 0..size {
            for x in 0..size {
                let color = if x < x0 || x >= x0 + sw || y < y0 || y >= y0 + sh {
                    BACKGROUND
                } else {
                    // Undo the printer's mirroring so the part shows the right way round.
                    let mut fx = (x - x0) * FOOT_W / sw;
                    let mut fy = (y - y0) * FOOT_H / sh;
                    if machine.mirror_x {
                        fx = FOOT_W - 1 - fx;
                    }
                    if machine.mirror_y {
                        fy = FOOT_H - 1 - fy;
                    }
                    if self.0[fy * FOOT_W + fx] {
                        PRINT
                    } else {
                        SCREEN
                    }
                };
                out.extend_from_slice(&color.to_be_bytes());
            }
        }
        out
    }
}

const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

/// The current time in UTC as "YYYY-MM-DD HH:MM:SS".
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_utc(secs)
}

fn format_utc(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_times() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(1_794_802_403), "2026-11-16 04:13:23");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn text_fields_are_zero_padded_and_terminated() {
        let mut b = Bytes(Vec::new());
        b.text("$", 8);
        b.text("0123456789", 4);
        assert_eq!(b.0, [b"$\0\0\0\0\0\0\0".as_slice(), b"012\0"].concat());
    }
}
