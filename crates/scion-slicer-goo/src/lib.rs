//! ELEGOO `.goo` print files, written the way ELEGOO SatelLite writes them.
//!
//! The layout follows Elegoo's published "Goo Format Spec V1.2". What goes into each
//! field follows SatelLite 1.0.2.43's own output: the reference slices and the rules they
//! show are in `corpus/satellite/`, and `docs/design/goo-writer.md` explains the design.
//!
//! ```no_run
//! use scion_slicer_goo::{FileInfo, GooWriter, Machine, Resin};
//! # fn layers() -> Vec<Vec<u8>> { Vec::new() }
//! # fn resin() -> Resin { unimplemented!() }
//! let file = std::fs::File::create("part.goo")?;
//! let mut writer = GooWriter::new(file, Machine::saturn_4_ultra_16k(), resin(), FileInfo::scion_slicer())?;
//! for pixels in layers() {
//!     writer.add_layer(&pixels)?;
//! }
//! let (_file, summary) = writer.finish(None)?;
//! println!("{} layers, {} s", summary.layers, summary.print_time_s);
//! # Ok::<(), std::io::Error>(())
//! ```

pub mod read;
pub mod rle;
pub mod satellite;
pub mod settings;
pub mod writer;

pub use read::GooFile;
pub use settings::{LayerSettings, Machine, Motion, Resin, Waits};
pub use writer::{prepare_layer, FileInfo, GooWriter, PreparedLayer, Summary};
