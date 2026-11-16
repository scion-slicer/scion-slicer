//! Reading a `.goo` file back: the header's settings and each layer's record and data.

use crate::writer::{FOOTER, HEADER_SIZE, PARAMS_OFFSET};

/// The header, field by field, in the spec's order. Text fields stop at the first zero.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub version: String,
    pub software: String,
    pub software_version: String,
    pub created: String,
    pub printer_name: String,
    pub printer_type: String,
    pub profile_name: String,
    pub anti_aliasing: u16,
    pub grey_level: u16,
    pub blur_level: u16,
    pub layers: u32,
    pub resolution_x: u16,
    pub resolution_y: u16,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub display_width_mm: f32,
    pub display_height_mm: f32,
    pub machine_z_mm: f32,
    pub layer_height_mm: f32,
    pub exposure_s: f32,
    pub delay_mode: u8,
    pub light_off_s: f32,
    /// Bottom then normal: before lift, after lift, after retract (the spec's names).
    pub bottom_waits_s: [f32; 3],
    pub waits_s: [f32; 3],
    pub bottom_exposure_s: f32,
    pub bottom_layers: u32,
    /// Bottom lift distance and speed, lift, bottom retract, retract: 8 values.
    pub motion: [f32; 8],
    /// The same for the second stage.
    pub motion_second_stage: [f32; 8],
    pub bottom_pwm: u16,
    pub pwm: u16,
    pub advance_mode: u8,
    pub print_time_s: u32,
    pub volume_mm3: f32,
    pub weight_g: f32,
    pub cost: f32,
    pub currency: String,
    pub layer_content_offset: u32,
    pub gray_scale_level: u8,
    pub transition_layers: u16,
}

/// One layer's record, with its data still encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer<'a> {
    /// The 66-byte record, from the pause flag through the delimiter.
    pub record: &'a [u8],
    pub pause: u16,
    pub pause_position_z_mm: f32,
    pub z_mm: f32,
    pub exposure_s: f32,
    pub light_off_s: f32,
    /// Before lift, after lift, after retract.
    pub waits_s: [f32; 3],
    /// Lift, second lift, retract, second retract: distance and speed each.
    pub motion: [f32; 8],
    pub pwm: u16,
    /// The encoded image, with its 0x55 and checksum.
    pub data: &'a [u8],
}

/// A whole `.goo` file, parsed.
#[derive(Debug, Clone)]
pub struct GooFile<'a> {
    pub header: Header,
    pub layers: Vec<Layer<'a>>,
    pub footer: &'a [u8],
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at + n;
        let s = self
            .b
            .get(self.at..end)
            .ok_or_else(|| format!("file ends at byte {}", self.b.len()))?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
    fn text(&mut self, n: usize) -> Result<String, String> {
        let raw = self.take(n)?;
        let end = raw.iter().position(|&c| c == 0).unwrap_or(n);
        Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
    }
    fn crlf(&mut self, what: &str) -> Result<(), String> {
        let at = self.at;
        if self.take(2)? != b"\r\n" {
            return Err(format!("no delimiter after {what} at byte {at}"));
        }
        Ok(())
    }
}

impl<'a> GooFile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        let mut r = Reader { b: bytes, at: 0 };
        let version = r.text(4)?;
        r.take(8)?; // magic
        let software = r.text(32)?;
        let software_version = r.text(24)?;
        let created = r.text(24)?;
        let printer_name = r.text(32)?;
        let printer_type = r.text(32)?;
        let profile_name = r.text(32)?;
        let (anti_aliasing, grey_level, blur_level) = (r.u16()?, r.u16()?, r.u16()?);
        r.take(116 * 116 * 2)?;
        r.crlf("the small preview")?;
        r.take(290 * 290 * 2)?;
        r.crlf("the big preview")?;
        debug_assert_eq!(r.at, PARAMS_OFFSET);
        let header = Header {
            version,
            software,
            software_version,
            created,
            printer_name,
            printer_type,
            profile_name,
            anti_aliasing,
            grey_level,
            blur_level,
            layers: r.u32()?,
            resolution_x: r.u16()?,
            resolution_y: r.u16()?,
            mirror_x: r.u8()? != 0,
            mirror_y: r.u8()? != 0,
            display_width_mm: r.f32()?,
            display_height_mm: r.f32()?,
            machine_z_mm: r.f32()?,
            layer_height_mm: r.f32()?,
            exposure_s: r.f32()?,
            delay_mode: r.u8()?,
            light_off_s: r.f32()?,
            bottom_waits_s: r.f32s()?,
            waits_s: r.f32s()?,
            bottom_exposure_s: r.f32()?,
            bottom_layers: r.u32()?,
            motion: r.f32s()?,
            motion_second_stage: r.f32s()?,
            bottom_pwm: r.u16()?,
            pwm: r.u16()?,
            advance_mode: r.u8()?,
            print_time_s: r.u32()?,
            volume_mm3: r.f32()?,
            weight_g: r.f32()?,
            cost: r.f32()?,
            currency: r.text(8)?,
            layer_content_offset: r.u32()?,
            gray_scale_level: r.u8()?,
            transition_layers: r.u16()?,
        };
        if r.at != HEADER_SIZE {
            return Err(format!(
                "header ends at byte {}, expected {HEADER_SIZE}",
                r.at
            ));
        }
        r.at = header.layer_content_offset as usize;

        let mut layers = Vec::with_capacity(header.layers as usize);
        for i in 0..header.layers {
            let start = r.at;
            let pause = r.u16()?;
            let pause_position_z_mm = r.f32()?;
            let z_mm = r.f32()?;
            let exposure_s = r.f32()?;
            let light_off_s = r.f32()?;
            let waits_s = r.f32s()?;
            let motion = r.f32s()?;
            let pwm = r.u16()?;
            r.crlf(&format!("layer {i}'s record"))?;
            let record = &bytes[start..r.at];
            let len = r.u32()? as usize;
            let data = r.take(len)?;
            r.crlf(&format!("layer {i}'s data"))?;
            layers.push(Layer {
                record,
                pause,
                pause_position_z_mm,
                z_mm,
                exposure_s,
                light_off_s,
                waits_s,
                motion,
                pwm,
                data,
            });
        }
        let footer = &bytes[r.at..];
        if footer != FOOTER {
            return Err(format!(
                "the file doesn't end with the .goo footer ({} bytes left)",
                footer.len()
            ));
        }
        Ok(Self {
            header,
            layers,
            footer,
        })
    }
}
