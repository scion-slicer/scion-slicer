//! Reading the printers and resin profiles ELEGOO SatelLite keeps on this computer.
//!
//! SatelLite stores them in `ELEGOO SatelLite/My Machines/machines.json` under the
//! application-data folder (on macOS it creates `~/AppData/Roaming` for this). Each
//! printer has its fixed facts and a list of resin profiles, including the lift and
//! retract settings the Parameters tab doesn't show, which SatelLite uses for its print
//! time estimate.
//!
//! SatelLite's key names don't say what its UI says. Lined up against the UI for the
//! default "Normal" profile and SirayaTech Fast (Navy Grey):
//!
//! | UI (Parameters tab)          | key under `vd_printer_printer_para`              |
//! |------------------------------|--------------------------------------------------|
//! | Bottom Wait Before Cure      | `vd_printer_printer_bottom_wait_after_retract`  |
//! | Bottom Wait Before Release   | `vd_printer_printer_bottom_wait_before_lift`    |
//! | Bottom Wait After Release    | `vd_printer_printer_bottom_wait_after_lift`     |
//! | Normal Wait Before Cure      | `vd_printer_printer_para_bottom_lightoff_delay` |
//! | Normal Wait Before Release   | `vd_printer_printer_para_wait_after_cure` (0 in every profile seen; unconfirmed) |
//! | Normal Wait After Release    | `vd_printer_printer_para_lightoff_delay`        |

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::settings::{LayerSettings, Machine, Motion, Resin, Waits};

/// One printer in SatelLite, with its resin profiles.
#[derive(Debug, Clone)]
pub struct SatellitePrinter {
    pub machine: Machine,
    pub resins: Vec<Resin>,
    /// The profile SatelLite last applied, if any.
    pub active_resin: Option<String>,
}

/// Where SatelLite keeps its printers on this computer, if it's there.
pub fn default_machines_path() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("AppData/Roaming")),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|base| base.join("ELEGOO SatelLite/My Machines/machines.json"))
        .find(|p| p.is_file())
}

/// Read every printer and resin profile in a SatelLite `machines.json`.
pub fn load_machines(path: &Path) -> Result<Vec<SatellitePrinter>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_machines(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse_machines(json: &str) -> Result<Vec<SatellitePrinter>, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let plats = root["plattree"]
        .as_array()
        .ok_or("no printers (plattree) in the file")?;
    plats
        .iter()
        .map(|plat| {
            let item = &plat["platitem"];
            let printer = &item["vd_printer"];
            let machine = machine(&printer["vd_printer_machine_para"], &item["size"])?;
            let resins = printer["vd_printer_resin_paras"]
                .as_array()
                .map(|list| list.iter().filter_map(resin).collect())
                .unwrap_or_default();
            let active_resin = printer["vd_active_printer"]["vd_resine_profilename"]
                .as_str()
                .map(str::to_owned);
            Ok(SatellitePrinter {
                machine,
                resins,
                active_resin,
            })
        })
        .collect()
}

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn machine(m: &Value, size: &Value) -> Result<Machine, String> {
    let name = m["vd_printer_machine_para_type"]
        .as_str()
        .ok_or("a printer has no name")?
        .to_owned();
    let incline = m["vd_printer_machine_para_is_incline"]
        .as_bool()
        .unwrap_or(false);
    Ok(Machine {
        resolution_x: num(m, "vd_printer_machine_para_resx") as u16,
        resolution_y: num(m, "vd_printer_machine_para_resy") as u16,
        display_width_mm: num(size, "x") as f32,
        display_height_mm: num(size, "y") as f32,
        machine_z_mm: num(size, "z") as f32,
        mirror_x: m["vd_printer_machine_para_mirror_x"]
            .as_bool()
            .unwrap_or(false),
        mirror_y: m["vd_printer_machine_para_mirror_y"]
            .as_bool()
            .unwrap_or(false),
        // Measured on the Saturn 4 Ultra 16K, a tilting printer. Other printers will need
        // their own reference slices; until then tilting ones get the same and others none.
        tilt_seconds_per_layer: if incline { 2.5 } else { 0.0 },
        pause_position_z_mm: 200.0,
        name,
    })
}

fn resin(r: &Value) -> Option<Resin> {
    let name = r["vd_resine_profilename"].as_str()?.to_owned();
    let p = &r["vd_printer_printer_para"];
    let adv = &r["vd_printer_advanced_para"];
    let res = &r["vd_printer_resin_para"];
    let k = |key: &str| num(p, &format!("vd_printer_printer_{key}"));
    let pwm = |key: &str| {
        let v = num(adv, key);
        if v > 0.0 {
            v as u16
        } else {
            255
        }
    };
    let bottom_motion = Motion {
        lift_mm: k("para_bootom_lift_dis"),
        lift_speed_mm_min: k("para_bottom_lift_speed"),
        retract_mm: k("para_bootom_lift_dis"),
        retract_speed_mm_min: k("para_bottom_back_speed"),
    };
    let motion = Motion {
        lift_mm: k("para_lift_dis"),
        lift_speed_mm_min: k("para_lift_speed"),
        retract_mm: k("para_lift_dis"),
        retract_speed_mm_min: k("para_back_speed"),
    };
    Some(Resin {
        profile_name: name,
        layer_height_mm: k("para_thickness") as f32,
        bottom_layers: k("para_bottom_layer") as u32,
        transition_layers: k("para_tanslate_layer_count") as u16,
        bottom: LayerSettings {
            exposure_s: k("para_bottom_exposure_time") as f32,
            waits: Waits {
                before_cure_s: k("bottom_wait_after_retract") as f32,
                before_release_s: k("bottom_wait_before_lift") as f32,
                after_release_s: k("bottom_wait_after_lift") as f32,
            },
            pwm: pwm("vd_printer_advanced_para_bottom_led_power"),
            motion: bottom_motion,
        },
        normal: LayerSettings {
            exposure_s: k("para_exposure_time") as f32,
            waits: Waits {
                before_cure_s: k("para_bottom_lightoff_delay") as f32,
                before_release_s: k("para_wait_after_cure") as f32,
                after_release_s: k("para_lightoff_delay") as f32,
            },
            pwm: pwm("vd_printer_advanced_para_led_power"),
            motion,
        },
        density_g_ml: num(res, "vd_printer_resin_para_density") as f32,
        price_per_kg: num(res, "vd_printer_resin_para_cost") as f32,
        currency: "$".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two profiles whose settings were read off SatelLite's UI, trimmed to the keys
    /// that are read.
    const SAMPLE: &str = r#"{"plattree":[{"platitem":{
      "size":{"x":211.67999267578125,"y":118.37000274658203,"z":220},
      "vd_printer":{
        "vd_active_printer":{"vd_resine_profilename":"SirayaTech - Fast (Navy Grey)"},
        "vd_printer_machine_para":{"vd_printer_machine_para_type":"ELEGOO Saturn 4 Ultra 16K",
          "vd_printer_machine_para_resx":15120,"vd_printer_machine_para_resy":6230,
          "vd_printer_machine_para_mirror_x":false,"vd_printer_machine_para_mirror_y":true,
          "vd_printer_machine_para_is_incline":true},
        "vd_printer_resin_paras":[
          {"vd_resine_profilename":"Normal",
           "vd_printer_advanced_para":{"vd_printer_advanced_para_bottom_led_power":255,"vd_printer_advanced_para_led_power":255},
           "vd_printer_printer_para":{"vd_printer_printer_bottom_wait_after_lift":0,"vd_printer_printer_bottom_wait_after_retract":1.2000000476837158,
             "vd_printer_printer_bottom_wait_before_lift":0,"vd_printer_printer_para_back_speed":0,"vd_printer_printer_para_bootom_lift_dis":0,
             "vd_printer_printer_para_bottom_back_speed":0,"vd_printer_printer_para_bottom_exposure_time":32,"vd_printer_printer_para_bottom_layer":5,
             "vd_printer_printer_para_bottom_lift_speed":0,"vd_printer_printer_para_bottom_lightoff_delay":1.2000000476837158,
             "vd_printer_printer_para_exposure_time":2.299999952316284,"vd_printer_printer_para_lift_dis":0,"vd_printer_printer_para_lift_speed":0,
             "vd_printer_printer_para_lightoff_delay":0,"vd_printer_printer_para_tanslate_layer_count":10,
             "vd_printer_printer_para_thickness":0.05000000074505806,"vd_printer_printer_para_wait_after_cure":0},
           "vd_printer_resin_para":{"vd_printer_resin_para_cost":30,"vd_printer_resin_para_density":1.149999976158142}},
          {"vd_resine_profilename":"SirayaTech - Fast (Navy Grey)",
           "vd_printer_advanced_para":{"vd_printer_advanced_para_bottom_led_power":255,"vd_printer_advanced_para_led_power":255},
           "vd_printer_printer_para":{"vd_printer_printer_bottom_wait_after_lift":0,"vd_printer_printer_bottom_wait_after_retract":1,
             "vd_printer_printer_bottom_wait_before_lift":1.5,"vd_printer_printer_para_back_speed":150,"vd_printer_printer_para_bootom_lift_dis":5,
             "vd_printer_printer_para_bottom_back_speed":150,"vd_printer_printer_para_bottom_exposure_time":30,"vd_printer_printer_para_bottom_layer":5,
             "vd_printer_printer_para_bottom_lift_speed":65,"vd_printer_printer_para_bottom_lightoff_delay":0,
             "vd_printer_printer_para_exposure_time":1.899999976158142,"vd_printer_printer_para_lift_dis":5,"vd_printer_printer_para_lift_speed":65,
             "vd_printer_printer_para_lightoff_delay":1,"vd_printer_printer_para_tanslate_layer_count":10,
             "vd_printer_printer_para_thickness":0.05000000074505806,"vd_printer_printer_para_wait_after_cure":0},
           "vd_printer_resin_para":{"vd_printer_resin_para_cost":30,"vd_printer_resin_para_density":1.149999976158142}}
        ]}}}]}"#;

    #[test]
    fn reads_the_printer_as_satellite_describes_it() {
        let printers = parse_machines(SAMPLE).unwrap();
        assert_eq!(printers.len(), 1);
        assert_eq!(printers[0].machine, Machine::saturn_4_ultra_16k());
        assert_eq!(
            printers[0].active_resin.as_deref(),
            Some("SirayaTech - Fast (Navy Grey)")
        );
    }

    #[test]
    fn maps_the_waits_as_the_ui_shows_them() {
        let printers = parse_machines(SAMPLE).unwrap();
        let (normal, navy) = (&printers[0].resins[0], &printers[0].resins[1]);
        // What SatelLite's Parameters tab shows for each.
        assert_eq!(normal.bottom.exposure_s, 32.0);
        assert_eq!(
            normal.bottom.waits,
            Waits {
                before_cure_s: 1.2,
                before_release_s: 0.0,
                after_release_s: 0.0
            }
        );
        assert_eq!(normal.normal.exposure_s, 2.3);
        assert_eq!(
            normal.normal.waits,
            Waits {
                before_cure_s: 1.2,
                before_release_s: 0.0,
                after_release_s: 0.0
            }
        );
        assert_eq!(
            navy.bottom.waits,
            Waits {
                before_cure_s: 1.0,
                before_release_s: 1.5,
                after_release_s: 0.0
            }
        );
        assert_eq!(
            navy.normal.waits,
            Waits {
                before_cure_s: 0.0,
                before_release_s: 0.0,
                after_release_s: 1.0
            }
        );
        assert_eq!(navy.normal.exposure_s, 1.9);
        assert_eq!(navy.density_g_ml, 1.15);
        // The hidden motion that makes the print-time estimate 9.1 s a layer slower.
        assert!((navy.normal.motion.seconds() - (5.0 / 65.0 + 5.0 / 150.0) * 60.0).abs() < 1e-9);
        assert_eq!(normal.normal.motion.seconds(), 0.0);
    }
}
