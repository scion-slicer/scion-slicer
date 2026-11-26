//! Encrypted CHITUBOX `.ctb` (version 5) print files, the format Chitubox Pro writes for
//! per-layer settings ("Slicing by Height") and that Elegoo's printers read.
//!
//! The record layouts follow UVtools; the encryption follows DragonFruit's MIT-licensed
//! CTB plugin. What goes into each field follows Chitubox's own output.

pub mod crypto;
pub mod fields;
pub mod read;
pub mod rle;
pub mod write;

pub use read::CtbFile;
pub use write::{
    write, FileSettings, Layer, LayerSettings, Machine, Motion, Preview, ResinInfo, Summary,
};
