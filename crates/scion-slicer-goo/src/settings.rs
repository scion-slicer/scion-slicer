//! What goes into a `.goo` file besides the layers: the printer and the resin profile.

/// A printer's fixed facts, plus the constants SatelLite uses for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    /// Written as both the printer name and the printer type, as SatelLite does.
    pub name: String,
    pub resolution_x: u16,
    pub resolution_y: u16,
    /// The printable area of the screen, in mm.
    pub display_width_mm: f32,
    pub display_height_mm: f32,
    /// How far the plate can travel, in mm.
    pub machine_z_mm: f32,
    /// Whether the layer images are mirrored for this printer. Only recorded in the
    /// header: the caller mirrors the images.
    pub mirror_x: bool,
    pub mirror_y: bool,
    /// Time SatelLite adds to every layer for the tilting vat, in seconds.
    pub tilt_seconds_per_layer: f64,
    /// What SatelLite writes as every layer's pause position, in mm.
    pub pause_position_z_mm: f32,
}

impl Machine {
    /// The ELEGOO Saturn 4 Ultra 16K, as SatelLite 1.0.2.43 describes it.
    pub fn saturn_4_ultra_16k() -> Self {
        Self {
            name: "ELEGOO Saturn 4 Ultra 16K".into(),
            resolution_x: 15120,
            resolution_y: 6230,
            display_width_mm: 211.68,
            display_height_mm: 118.37,
            machine_z_mm: 220.0,
            mirror_x: false,
            mirror_y: true,
            tilt_seconds_per_layer: 2.5,
            pause_position_z_mm: 200.0,
        }
    }

    pub fn pixel_count(&self) -> usize {
        self.resolution_x as usize * self.resolution_y as usize
    }

    /// The area of one pixel in mm².
    pub fn pixel_area_mm2(&self) -> f64 {
        (self.display_width_mm as f64 / self.resolution_x as f64)
            * (self.display_height_mm as f64 / self.resolution_y as f64)
    }
}

/// The waits around one layer's exposure, named as SatelLite's Parameters tab names them.
///
/// The spec's names are in brackets: "Wait Before Release" is the spec's "before lift",
/// "Wait After Release" its "after lift", and "Wait Before Cure" its "after retract".
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Waits {
    pub before_cure_s: f32,
    pub before_release_s: f32,
    pub after_release_s: f32,
}

/// How the plate moves between layers. SatelLite doesn't write these to the file (it
/// writes 0.05 placeholders); they only feed the print-time estimate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    pub lift_mm: f64,
    pub lift_speed_mm_min: f64,
    pub retract_mm: f64,
    pub retract_speed_mm_min: f64,
}

impl Motion {
    /// Seconds spent lifting and retracting.
    pub fn seconds(&self) -> f64 {
        let leg = |mm: f64, speed: f64| if speed > 0.0 { mm / speed * 60.0 } else { 0.0 };
        leg(self.lift_mm, self.lift_speed_mm_min) + leg(self.retract_mm, self.retract_speed_mm_min)
    }
}

/// The settings for one kind of layer: bottom or normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerSettings {
    pub exposure_s: f32,
    pub waits: Waits,
    /// Light power, 0–255.
    pub pwm: u16,
    pub motion: Motion,
}

/// A resin profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Resin {
    pub profile_name: String,
    pub layer_height_mm: f32,
    pub bottom_layers: u32,
    /// Layers after the bottom ones whose exposure steps down to the normal exposure.
    /// They keep the bottom layers' waits.
    pub transition_layers: u16,
    pub bottom: LayerSettings,
    pub normal: LayerSettings,
    pub density_g_ml: f32,
    pub price_per_kg: f32,
    /// Up to 8 bytes, e.g. "$".
    pub currency: String,
}

impl Resin {
    /// SatelLite's default resin profile for the Saturn 4 Ultra 16K, "Normal".
    pub fn satellite_normal() -> Self {
        let layer = |exposure_s| LayerSettings {
            exposure_s,
            waits: Waits {
                before_cure_s: 1.2,
                before_release_s: 0.0,
                after_release_s: 0.0,
            },
            pwm: 255,
            motion: Motion::default(),
        };
        Self {
            profile_name: "Normal".into(),
            layer_height_mm: 0.05,
            bottom_layers: 5,
            transition_layers: 10,
            bottom: layer(32.0),
            normal: layer(2.3),
            density_g_ml: 1.15,
            price_per_kg: 30.0,
            currency: "$".into(),
        }
    }

    /// SatelLite's SirayaTech Fast (Navy Grey) profile for the Saturn 4 Ultra 16K, with the
    /// bottom Wait Before Release at 1.5 s, as used for the reference slices and test prints.
    pub fn siraya_fast_navy_grey() -> Self {
        let motion = Motion {
            lift_mm: 5.0,
            lift_speed_mm_min: 65.0,
            retract_mm: 5.0,
            retract_speed_mm_min: 150.0,
        };
        Self {
            profile_name: "SirayaTech - Fast (Navy Grey)".into(),
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
                motion,
            },
            normal: LayerSettings {
                exposure_s: 1.9,
                waits: Waits {
                    before_cure_s: 0.0,
                    before_release_s: 0.0,
                    after_release_s: 1.0,
                },
                pwm: 255,
                motion,
            },
            density_g_ml: 1.15,
            price_per_kg: 30.0,
            currency: "$".into(),
        }
    }
}

/// Which kind a layer is, by its index from the plate (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Bottom,
    /// The `step`-th transition layer, counting from 1.
    Transition {
        step: u32,
    },
    Normal,
}

impl Resin {
    pub fn layer_kind(&self, index: u32) -> LayerKind {
        let transition_end = self.bottom_layers + self.transition_layers as u32;
        if index < self.bottom_layers {
            LayerKind::Bottom
        } else if index < transition_end {
            LayerKind::Transition {
                step: index - self.bottom_layers + 1,
            }
        } else {
            LayerKind::Normal
        }
    }

    /// The exposure of a layer. Transition layers step down in equal steps of
    /// `(bottom − normal) / transition_layers`, so the last one is already at the normal
    /// exposure. SatelLite rounds the result to hundredths of a second.
    pub fn exposure_s(&self, index: u32) -> f32 {
        match self.layer_kind(index) {
            LayerKind::Bottom => self.bottom.exposure_s,
            LayerKind::Normal => self.normal.exposure_s,
            LayerKind::Transition { step } => {
                let bottom = self.bottom.exposure_s as f64;
                let normal = self.normal.exposure_s as f64;
                let per_step = (bottom - normal) / self.transition_layers as f64;
                round_to(bottom - step as f64 * per_step, 2) as f32
            }
        }
    }

    /// The settings that apply to a layer, apart from its exposure: the bottom ones for
    /// bottom and transition layers.
    pub fn settings_for(&self, index: u32) -> &LayerSettings {
        match self.layer_kind(index) {
            LayerKind::Bottom | LayerKind::Transition { .. } => &self.bottom,
            LayerKind::Normal => &self.normal,
        }
    }

    /// The height of a layer's top, in mm, rounded to thousandths as SatelLite does.
    pub fn layer_z_mm(&self, index: u32) -> f32 {
        round_to((index as f64 + 1.0) * self.layer_height_mm as f64, 3) as f32
    }
}

pub(crate) fn round_to(value: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resin(transition: u16) -> Resin {
        let layer = |exposure_s| LayerSettings {
            exposure_s,
            waits: Waits::default(),
            pwm: 255,
            motion: Motion::default(),
        };
        Resin {
            profile_name: "test".into(),
            layer_height_mm: 0.05,
            bottom_layers: 5,
            transition_layers: transition,
            bottom: layer(30.0),
            normal: layer(1.9),
            density_g_ml: 1.15,
            price_per_kg: 30.0,
            currency: "$".into(),
        }
    }

    #[test]
    fn transition_steps_down_to_the_normal_exposure() {
        let r = resin(10);
        let ramp: Vec<f32> = (4..16).map(|i| r.exposure_s(i)).collect();
        assert_eq!(
            ramp,
            [30.0, 27.19, 24.38, 21.57, 18.76, 15.95, 13.14, 10.33, 7.52, 4.71, 1.9, 1.9]
        );
        let r = resin(0);
        assert_eq!(r.exposure_s(5), 1.9);
    }

    #[test]
    fn transition_layers_use_bottom_settings() {
        let r = resin(10);
        assert_eq!(r.layer_kind(14), LayerKind::Transition { step: 10 });
        assert!(std::ptr::eq(r.settings_for(14), &r.bottom));
        assert!(std::ptr::eq(r.settings_for(15), &r.normal));
    }

    #[test]
    fn layer_heights_are_rounded() {
        let r = resin(10);
        assert_eq!(r.layer_z_mm(0), 0.05);
        assert_eq!(r.layer_z_mm(14), 0.75);
        assert_eq!(r.layer_z_mm(2369), 118.5);
    }
}
