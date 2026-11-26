//! Writing an encrypted CTB (v5) file laid out the way Chitubox Pro lays it out.
//!
//! In order: the 48-byte header; the 288-byte settings block (AES-encrypted); the large
//! and small previews, each a 16-byte record followed by its image; the machine name; the
//! 320-byte disclaimer; the resin block; the layer pointer table; each layer's 88-byte
//! record followed by its image; 8 bytes of zeros; the encrypted signature; and a CRC-32
//! of everything before it.

use crate::crypto;
use crate::fields::{Record, HEADER, LAYER_DEF, SETTINGS};
use crate::read::MAGIC_ENCRYPTED;

/// The text Chitubox puts in every CTB file after the machine name.
pub const DISCLAIMER: &str = "Layout and record format for the ctb and cbddlp file types are the copyrighted programs or codes of CBD Technology (China) Inc..The Customer or User shall not in any manner reproduce, distribute, modify, decompile, disassemble, decrypt, extract, reverse engineer, lease, assign, or sublicense the said programs or codes.";
pub const DISCLAIMER_SIZE: usize = 320;
/// Values Chitubox writes in settings fields whose meaning isn't known. `Unknown7`
/// varies by file (37335 for the mold top, 5071 for the test pins); we write the first.
const UNKNOWN7: u32 = 37335;
const UNKNOWN8: u32 = 3;

/// How the plate moves after a layer, as a CTB layer record stores it. Chitubox's
/// tilting-vat profiles use two stages each way.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    pub lift_mm: f32,
    pub lift_speed_mm_min: f32,
    pub lift2_mm: f32,
    pub lift2_speed_mm_min: f32,
    /// The first retract stage's speed; its distance is the total lift less `retract2_mm`.
    pub retract_speed_mm_min: f32,
    pub retract2_mm: f32,
    pub retract2_speed_mm_min: f32,
}

impl Motion {
    pub fn total_lift_mm(&self) -> f32 {
        self.lift_mm + self.lift2_mm
    }

    /// Seconds spent moving: both lift stages and both retract stages.
    pub fn seconds(&self) -> f64 {
        let leg = |mm: f32, speed: f32| {
            if speed > 0.0 {
                mm as f64 / speed as f64 * 60.0
            } else {
                0.0
            }
        };
        let retract1 = (self.total_lift_mm() - self.retract2_mm).max(0.0);
        leg(self.lift_mm, self.lift_speed_mm_min)
            + leg(self.lift2_mm, self.lift2_speed_mm_min)
            + leg(retract1, self.retract_speed_mm_min)
            + leg(self.retract2_mm, self.retract2_speed_mm_min)
    }
}

/// Everything a layer record holds apart from where the layer is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayerSettings {
    pub exposure_s: f32,
    pub light_off_s: f32,
    /// Chitubox: "rest time before lift" (after the exposure).
    pub rest_before_lift_s: f32,
    pub rest_after_lift_s: f32,
    /// Chitubox: "rest time after retract" (before the next exposure).
    pub rest_after_retract_s: f32,
    pub pwm: u8,
    pub motion: Motion,
}

impl LayerSettings {
    /// Seconds the layer takes: exposure, light-off, rests and motion.
    pub fn seconds(&self) -> f64 {
        self.exposure_s as f64
            + self.light_off_s as f64
            + self.rest_before_lift_s as f64
            + self.rest_after_lift_s as f64
            + self.rest_after_retract_s as f64
            + self.motion.seconds()
    }
}

/// One layer to write.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Height of the layer's top, as written.
    pub z_mm: f32,
    pub settings: LayerSettings,
    /// The image, run-length encoded with [`crate::rle::encode`], not yet scrambled.
    pub data: Vec<u8>,
}

/// The printer.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub name: String,
    pub resolution_x: u32,
    pub resolution_y: u32,
    pub display_width_mm: f32,
    pub display_height_mm: f32,
    pub machine_z_mm: f32,
}

impl Machine {
    pub fn saturn_4_ultra_16k() -> Self {
        Self {
            name: "ELEGOO Saturn 4 Ultra 16K".into(),
            resolution_x: 15120,
            resolution_y: 6230,
            display_width_mm: 211.68,
            display_height_mm: 118.37,
            machine_z_mm: 220.0,
        }
    }
}

/// The resin, as the resin block names it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResinInfo {
    pub resin_type: String,
    pub resin_name: String,
    pub profile_name: String,
    pub density_g_ml: f32,
    pub price_per_kg: f32,
    /// RGBA.
    pub color: [u8; 4],
}

/// The file-wide settings: the header shows the first range's values, as Chitubox does.
#[derive(Debug, Clone, PartialEq)]
pub struct FileSettings {
    pub layer_height_mm: f32,
    pub bottom_layers: u32,
    pub transition_layers: u32,
    pub bottom: LayerSettings,
    pub normal: LayerSettings,
    /// Set the per-layer flag (0x40) so the printer uses each layer's own record.
    pub per_layer_settings: bool,
    pub anti_alias_level: u32,
    /// Scrambles the layer images; any nonzero value works (Chitubox picks a random one).
    pub layer_xor_key: u32,
    /// Seconds since 1970.
    pub modified_unix: u64,
    pub volume_ml: f32,
}

/// A preview image, row by row, RGB.
#[derive(Debug, Clone)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// What the written file says about the print.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub layers: u32,
    pub print_time_s: u32,
    pub volume_ml: f32,
    pub weight_g: f32,
    pub cost: f32,
    pub bytes: usize,
}

/// Write a whole encrypted CTB file.
pub fn write(
    machine: &Machine,
    resin: &ResinInfo,
    settings: &FileSettings,
    preview: &Preview,
    layers: &[Layer],
) -> Result<(Vec<u8>, Summary), String> {
    let mut out = vec![0u8; 48 + 288];

    // Previews: Chitubox writes the same picture twice, each as a 16-byte record
    // (width, height, image offset, image length) followed by the image.
    let image = encode_preview(preview);
    let mut preview_at = [0u32; 2];
    for slot in &mut preview_at {
        *slot = out.len() as u32;
        let image_at = out.len() as u32 + 16;
        for v in [preview.width, preview.height, image_at, image.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&image);
    }

    let machine_name_at = out.len() as u32;
    out.extend_from_slice(machine.name.as_bytes());

    let disclaimer_at = out.len() as u32;
    let mut disclaimer = DISCLAIMER.as_bytes().to_vec();
    disclaimer.resize(DISCLAIMER_SIZE, 0);
    out.extend_from_slice(&disclaimer);

    let resin_at = out.len() as u32;
    write_resin_block(&mut out, resin);

    let pointers_at = out.len();
    out.resize(out.len() + layers.len() * 16, 0);

    let mut time_s = 0.0f64;
    // Chitubox adds these up in 32-bit floats.
    let mut exposure_and_rests = 0.0f32;
    for (i, layer) in layers.iter().enumerate() {
        let def_at = out.len() as u64;
        let data_at = def_at + 88;
        let s = &layer.settings;
        let mut def = Record::zeroed(LAYER_DEF);
        def.set("TableSize", 88.0);
        def.set("PositionZ", layer.z_mm as f64);
        def.set("ExposureTime", s.exposure_s as f64);
        def.set("LightOffDelay", s.light_off_s as f64);
        def.set("LayerDataOffset", (data_at & 0xFFFF_FFFF) as f64);
        def.set("PageNumber", (data_at >> 32) as f64);
        def.set("LayerDataLength", layer.data.len() as f64);
        def.set("LiftHeight", s.motion.total_lift_mm() as f64);
        def.set("LiftSpeed", s.motion.lift_speed_mm_min as f64);
        def.set("LiftHeight2", s.motion.lift2_mm as f64);
        def.set("LiftSpeed2", s.motion.lift2_speed_mm_min as f64);
        def.set("RetractSpeed", s.motion.retract_speed_mm_min as f64);
        def.set("RetractHeight2", s.motion.retract2_mm as f64);
        def.set("RetractSpeed2", s.motion.retract2_speed_mm_min as f64);
        def.set("RestTimeBeforeLift", s.rest_before_lift_s as f64);
        def.set("RestTimeAfterLift", s.rest_after_lift_s as f64);
        def.set("RestTimeAfterRetract", s.rest_after_retract_s as f64);
        def.set("LightPWM", s.pwm as f64);
        out.extend_from_slice(&def.to_bytes(LAYER_DEF));

        let mut data = layer.data.clone();
        crypto::layer_xor(settings.layer_xor_key, i as u32, &mut data);
        out.extend_from_slice(&data);

        let p = pointers_at + i * 16;
        out[p..p + 4].copy_from_slice(&((def_at & 0xFFFF_FFFF) as u32).to_le_bytes());
        out[p + 4..p + 8].copy_from_slice(&((def_at >> 32) as u32).to_le_bytes());
        out[p + 8..p + 12].copy_from_slice(&88u32.to_le_bytes());

        time_s += s.seconds();
        exposure_and_rests +=
            s.exposure_s + s.rest_before_lift_s + s.rest_after_lift_s + s.rest_after_retract_s;
    }
    out.extend_from_slice(&[0u8; 8]);

    // The signature is the SHA-256 of the settings' checksum value (Chitubox writes 0).
    let checksum: u64 = 0;
    use sha2::Digest;
    let mut signature = sha2::Sha256::digest(checksum.to_le_bytes()).to_vec();
    crypto::encrypt(&mut signature)?;
    let signature_at = out.len() as u32;
    out.extend_from_slice(&signature);

    let weight = settings.volume_ml * resin.density_g_ml;
    let cost = weight * resin.price_per_kg / 1000.0;
    let summary = Summary {
        layers: layers.len() as u32,
        print_time_s: time_s.round() as u32,
        volume_ml: settings.volume_ml,
        weight_g: weight,
        cost,
        bytes: out.len() + 4,
    };

    let (b, n) = (&settings.bottom, &settings.normal);
    let mut st = Record::zeroed(SETTINGS);
    let pairs: Vec<(&str, f64)> = vec![
        ("ChecksumValue", checksum as f64),
        ("LayerPointersOffset", pointers_at as f64),
        ("DisplayWidth", machine.display_width_mm as f64),
        ("DisplayHeight", machine.display_height_mm as f64),
        ("MachineZ", machine.machine_z_mm as f64),
        ("LayerHeight", settings.layer_height_mm as f64),
        ("ExposureTime", n.exposure_s as f64),
        ("BottomExposureTime", b.exposure_s as f64),
        ("LightOffDelay", n.light_off_s as f64),
        ("BottomLayerCount", settings.bottom_layers as f64),
        ("ResolutionX", machine.resolution_x as f64),
        ("ResolutionY", machine.resolution_y as f64),
        ("LayerCount", layers.len() as f64),
        ("LargePreviewOffset", preview_at[0] as f64),
        ("SmallPreviewOffset", preview_at[1] as f64),
        ("PrintTime", summary.print_time_s as f64),
        ("ProjectorType", 1.0),
        ("BottomLiftHeight", b.motion.lift_mm as f64),
        ("BottomLiftSpeed", b.motion.lift_speed_mm_min as f64),
        ("LiftHeight", n.motion.lift_mm as f64),
        ("LiftSpeed", n.motion.lift_speed_mm_min as f64),
        ("RetractSpeed", n.motion.retract_speed_mm_min as f64),
        ("MaterialMilliliters", summary.volume_ml as f64),
        ("MaterialGrams", summary.weight_g as f64),
        ("MaterialCost", summary.cost as f64),
        ("BottomLightOffDelay", b.light_off_s as f64),
        ("Unknown3", 1.0),
        ("LightPWM", n.pwm as f64),
        ("BottomLightPWM", b.pwm as f64),
        ("LayerXorKey", settings.layer_xor_key as f64),
        ("RestTimeAfterLift", n.rest_before_lift_s as f64),
        ("MachineNameOffset", machine_name_at as f64),
        ("MachineNameSize", machine.name.len() as f64),
        ("AntiAliasFlag", 7.0),
        (
            "PerLayerSettings",
            if settings.per_layer_settings {
                64.0
            } else {
                0.0
            },
        ),
        (
            "ModifiedTimestampMinutes",
            (settings.modified_unix / 60) as f64,
        ),
        ("AntiAliasLevel", settings.anti_alias_level as f64),
        ("RestTimeAfterRetract", b.rest_after_retract_s as f64),
        ("RestTimeAfterLift2", b.rest_after_lift_s as f64),
        ("TransitionLayerCount", settings.transition_layers as f64),
        ("BottomRetractSpeed", b.motion.retract_speed_mm_min as f64),
        ("Four1", 4.0),
        ("Four2", 4.0),
        ("RestTimeAfterRetract2", n.rest_after_retract_s as f64),
        ("RestTimeAfterLift3", n.rest_after_lift_s as f64),
        ("RestTimeBeforeLift", b.rest_before_lift_s as f64),
        ("Unknown7", UNKNOWN7 as f64),
        ("Unknown8", UNKNOWN8 as f64),
        ("LastLayerIndex", layers.len() as f64),
        ("DisclaimerOffset", disclaimer_at as f64),
        ("DisclaimerSize", DISCLAIMER_SIZE as f64),
        ("Padding7", 1.0),
        ("ResinParametersAddress", resin_at as f64),
    ];
    for (k, v) in pairs {
        st.set(k, v);
    }
    // Unknown6 is the total exposure and rest time, stored as a float's bits.
    st.set("Unknown6", exposure_and_rests.to_bits() as f64);
    let mut settings_bytes = st.to_bytes(SETTINGS);
    crypto::encrypt(&mut settings_bytes)?;
    out[48..48 + 288].copy_from_slice(&settings_bytes);

    let mut header = Record::zeroed(HEADER);
    for (k, v) in [
        ("Magic", MAGIC_ENCRYPTED as f64),
        ("SettingsSize", 288.0),
        ("SettingsOffset", 48.0),
        ("Version", 5.0),
        ("SignatureSize", 32.0),
        ("SignatureOffset", signature_at as f64),
        ("Unknown4", 1.0),
        ("Unknown5", 1.0),
        ("Unknown7", 42.0),
    ] {
        header.set(k, v);
    }
    out[..48].copy_from_slice(&header.to_bytes(HEADER));

    let crc = crc32fast::hash(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    Ok((out, summary))
}

fn write_resin_block(out: &mut Vec<u8>, resin: &ResinInfo) {
    // 4 bytes of zeros, the colour, then offset and length of the type, name and profile
    // strings, the density, 4 bytes of zeros, and the three strings.
    let start = out.len() as u32 + 40;
    let strings = [&resin.resin_type, &resin.resin_name, &resin.profile_name];
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&resin.color);
    let mut at = start;
    for s in strings {
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        at += s.len() as u32;
    }
    out.extend_from_slice(&resin.density_g_ml.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for s in strings {
        out.extend_from_slice(s.as_bytes());
    }
}

/// CTB preview encoding: RGB555 words (bit 5 is the repeat flag), a repeated colour
/// followed by a 0x3000 | (count - 1) word.
pub fn encode_preview(p: &Preview) -> Vec<u8> {
    let word = |i: usize| {
        let (r, g, b) = (
            p.rgb[i * 3] as u16,
            p.rgb[i * 3 + 1] as u16,
            p.rgb[i * 3 + 2] as u16,
        );
        ((r >> 3) << 11) | ((g >> 3) << 6) | (b >> 3)
    };
    let n = (p.width * p.height) as usize;
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = word(i);
        let mut run = 1;
        while i + run < n && run < 0x1000 && word(i + run) == c {
            run += 1;
        }
        if run <= 2 {
            for _ in 0..run {
                out.extend_from_slice(&(c & !0x20).to_le_bytes());
            }
        } else {
            out.extend_from_slice(&(c | 0x20).to_le_bytes());
            out.extend_from_slice(&(0x3000 | ((run - 1) as u16 & 0x0FFF)).to_le_bytes());
        }
        i += run;
    }
    out
}
