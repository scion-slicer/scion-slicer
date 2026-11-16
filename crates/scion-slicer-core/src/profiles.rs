//! The printers and resin profiles slicing can use: ELEGOO SatelLite's, when it's installed
//! on this computer, otherwise the ones built in.

use scion_slicer_goo::satellite::{default_machines_path, load_machines, SatellitePrinter};
use scion_slicer_goo::{Machine, Resin};
use serde::Serialize;

/// Where the printers came from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// ELEGOO SatelLite's profiles, read from this file.
    Satellite { path: String },
    /// Built in: the Saturn 4 Ultra 16K with SatelLite's "Normal" and SirayaTech Fast
    /// (Navy Grey) profiles.
    BuiltIn,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub printers: Vec<SatellitePrinter>,
    pub source: Source,
    /// Why SatelLite's profiles weren't used, if they exist but couldn't be read.
    pub problem: Option<String>,
}

impl Catalog {
    /// SatelLite's printers if it's installed and has any, otherwise the built-in ones.
    pub fn load() -> Self {
        let mut problem = None;
        if let Some(path) = default_machines_path() {
            match load_machines(&path) {
                Ok(printers) if !printers.is_empty() => {
                    return Self {
                        printers,
                        source: Source::Satellite {
                            path: path.display().to_string(),
                        },
                        problem,
                    };
                }
                Ok(_) => problem = Some(format!("{} has no printers", path.display())),
                Err(e) => problem = Some(e),
            }
        }
        Self {
            printers: built_in(),
            source: Source::BuiltIn,
            problem,
        }
    }

    /// The printer called `name` (or containing it), or the first printer.
    pub fn printer(&self, name: Option<&str>) -> Result<&SatellitePrinter, String> {
        match name {
            None => self
                .printers
                .first()
                .ok_or_else(|| "no printers are set up".to_string()),
            Some(name) => pick(&self.printers, name, |p| &p.machine.name, "printer"),
        }
    }

    /// The resin profile called `name` (or containing it) on `printer`, or the one
    /// SatelLite last used, or the first.
    pub fn resin<'a>(
        &self,
        printer: &'a SatellitePrinter,
        name: Option<&str>,
    ) -> Result<&'a Resin, String> {
        match name {
            Some(name) => pick(&printer.resins, name, |r| &r.profile_name, "resin profile"),
            None => printer
                .active_resin
                .as_deref()
                .and_then(|active| printer.resins.iter().find(|r| r.profile_name == active))
                .or(printer.resins.first())
                .ok_or_else(|| format!("the {} has no resin profiles", printer.machine.name)),
        }
    }
}

fn built_in() -> Vec<SatellitePrinter> {
    vec![SatellitePrinter {
        machine: Machine::saturn_4_ultra_16k(),
        resins: vec![Resin::satellite_normal(), Resin::siraya_fast_navy_grey()],
        active_resin: Some(Resin::siraya_fast_navy_grey().profile_name),
    }]
}

/// An exact match ignoring case, else the only name containing `wanted`.
fn pick<'a, T>(
    items: &'a [T],
    wanted: &str,
    name: impl Fn(&T) -> &String,
    what: &str,
) -> Result<&'a T, String> {
    let lower = wanted.to_lowercase();
    if let Some(item) = items.iter().find(|i| name(i).to_lowercase() == lower) {
        return Ok(item);
    }
    let matches: Vec<&T> = items
        .iter()
        .filter(|i| name(i).to_lowercase().contains(&lower))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no {what} matches \"{wanted}\"; there are: {}",
            items
                .iter()
                .map(|i| name(i).as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )),
        many => Err(format!(
            "\"{wanted}\" matches more than one {what}: {}",
            many.iter()
                .map(|i| name(i).as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built_in_catalog() -> Catalog {
        Catalog {
            printers: built_in(),
            source: Source::BuiltIn,
            problem: None,
        }
    }

    #[test]
    fn finds_printers_and_resins_by_part_of_the_name() {
        let c = built_in_catalog();
        let p = c.printer(Some("16k")).unwrap();
        assert_eq!(
            c.resin(p, Some("navy")).unwrap().profile_name,
            "SirayaTech - Fast (Navy Grey)"
        );
        assert_eq!(c.resin(p, Some("normal")).unwrap().profile_name, "Normal");
        assert_eq!(
            c.resin(p, None).unwrap().profile_name,
            "SirayaTech - Fast (Navy Grey)"
        );
        assert!(c
            .resin(p, Some("tenacious"))
            .unwrap_err()
            .contains("there are"));
        assert!(c.resin(p, Some("a")).unwrap_err().contains("more than one"));
    }
}
