//! The fixed-size records of an encrypted CTB file, as tables of named fields.
//!
//! The names and order follow UVtools' `CTBEncryptedFile`, which reads Chitubox's files.
//! Keeping each record as a table lets the reader print every field by name and the
//! writer and the comparison work field by field, unknown fields included.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    U8,
    U16,
    U32,
    U64,
    F32,
}

impl Kind {
    pub fn size(self) -> usize {
        match self {
            Kind::U8 => 1,
            Kind::U16 => 2,
            Kind::U32 | Kind::F32 => 4,
            Kind::U64 => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Int(u64),
    Float(f32),
}

impl Value {
    pub fn as_f32(self) -> f32 {
        match self {
            Value::Int(v) => v as f32,
            Value::Float(v) => v,
        }
    }
    pub fn as_u64(self) -> u64 {
        match self {
            Value::Int(v) => v,
            Value::Float(v) => v as u64,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{v}"),
            Value::Float(v) => write!(f, "{v}"),
        }
    }
}

pub type Layout = &'static [(&'static str, Kind)];

use Kind::*;

/// The 48-byte file header (not encrypted).
pub const HEADER: Layout = &[
    ("Magic", U32),
    ("SettingsSize", U32),
    ("SettingsOffset", U32),
    ("Unknown1", U32),
    ("Version", U32),
    ("SignatureSize", U32),
    ("SignatureOffset", U32),
    ("Unknown", U32),
    ("Unknown4", U16),
    ("Unknown5", U16),
    ("Unknown6", U32),
    ("Unknown7", U32),
    ("Unknown8", U32),
];

/// The 288-byte settings block (AES-encrypted in the file).
pub const SETTINGS: Layout = &[
    ("ChecksumValue", U64),
    ("LayerPointersOffset", U32),
    ("DisplayWidth", F32),
    ("DisplayHeight", F32),
    ("MachineZ", F32),
    ("Unknown1", U32),
    ("Unknown2", U32),
    ("TotalHeightMillimeter", F32),
    ("LayerHeight", F32),
    ("ExposureTime", F32),
    ("BottomExposureTime", F32),
    ("LightOffDelay", F32),
    ("BottomLayerCount", U32),
    ("ResolutionX", U32),
    ("ResolutionY", U32),
    ("LayerCount", U32),
    ("LargePreviewOffset", U32),
    ("SmallPreviewOffset", U32),
    ("PrintTime", U32),
    ("ProjectorType", U32),
    ("BottomLiftHeight", F32),
    ("BottomLiftSpeed", F32),
    ("LiftHeight", F32),
    ("LiftSpeed", F32),
    ("RetractSpeed", F32),
    ("MaterialMilliliters", F32),
    ("MaterialGrams", F32),
    ("MaterialCost", F32),
    ("BottomLightOffDelay", F32),
    ("Unknown3", U32),
    ("LightPWM", U16),
    ("BottomLightPWM", U16),
    ("LayerXorKey", U32),
    ("BottomLiftHeight2", F32),
    ("BottomLiftSpeed2", F32),
    ("LiftHeight2", F32),
    ("LiftSpeed2", F32),
    ("RetractHeight2", F32),
    ("RetractSpeed2", F32),
    ("RestTimeAfterLift", F32),
    ("MachineNameOffset", U32),
    ("MachineNameSize", U32),
    ("AntiAliasFlag", U8),
    ("Padding", U16),
    ("PerLayerSettings", U8),
    ("ModifiedTimestampMinutes", U32),
    ("AntiAliasLevel", U32),
    ("RestTimeAfterRetract", F32),
    ("RestTimeAfterLift2", F32),
    ("TransitionLayerCount", U32),
    ("BottomRetractSpeed", F32),
    ("BottomRetractSpeed2", F32),
    ("Padding1", U32),
    ("Four1", F32),
    ("Padding2", U32),
    ("Four2", F32),
    ("RestTimeAfterRetract2", F32),
    ("RestTimeAfterLift3", F32),
    ("RestTimeBeforeLift", F32),
    ("BottomRetractHeight2", F32),
    ("Unknown6", U32),
    ("Unknown7", U32),
    ("Unknown8", U32),
    ("LastLayerIndex", U32),
    ("Padding3", U32),
    ("Padding4", U32),
    ("Padding5", U32),
    ("Padding6", U32),
    ("DisclaimerOffset", U32),
    ("DisclaimerSize", U32),
    ("Padding7", U32),
    ("ResinParametersAddress", U32),
    ("Padding8", U32),
    ("Padding9", U32),
];

/// The 88-byte record before each layer's image data.
pub const LAYER_DEF: Layout = &[
    ("TableSize", U32),
    ("PositionZ", F32),
    ("ExposureTime", F32),
    ("LightOffDelay", F32),
    ("LayerDataOffset", U32),
    ("PageNumber", U32),
    ("LayerDataLength", U32),
    ("Unknown3", U32),
    ("EncryptedDataOffset", U32),
    ("EncryptedDataLength", U32),
    ("LiftHeight", F32),
    ("LiftSpeed", F32),
    ("LiftHeight2", F32),
    ("LiftSpeed2", F32),
    ("RetractSpeed", F32),
    ("RetractHeight2", F32),
    ("RetractSpeed2", F32),
    ("RestTimeBeforeLift", F32),
    ("RestTimeAfterLift", F32),
    ("RestTimeAfterRetract", F32),
    ("LightPWM", F32),
    ("Unknown6", U32),
];

/// The 32-byte record before each preview image.
pub const PREVIEW: Layout = &[
    ("ResolutionX", U32),
    ("ResolutionY", U32),
    ("ImageOffset", U32),
    ("ImageLength", U32),
    ("Unknown1", U32),
    ("Unknown2", U32),
    ("Unknown3", U32),
    ("Unknown4", U32),
];

pub fn size(layout: Layout) -> usize {
    layout.iter().map(|(_, k)| k.size()).sum()
}

/// A parsed record: each field's name and value, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub fields: Vec<(&'static str, Value)>,
}

impl Record {
    pub fn parse(layout: Layout, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < size(layout) {
            return Err(format!(
                "record needs {} bytes, got {}",
                size(layout),
                bytes.len()
            ));
        }
        let mut at = 0;
        let mut fields = Vec::with_capacity(layout.len());
        for &(name, kind) in layout {
            let b = &bytes[at..at + kind.size()];
            let value = match kind {
                U8 => Value::Int(b[0] as u64),
                U16 => Value::Int(u16::from_le_bytes(b.try_into().unwrap()) as u64),
                U32 => Value::Int(u32::from_le_bytes(b.try_into().unwrap()) as u64),
                U64 => Value::Int(u64::from_le_bytes(b.try_into().unwrap())),
                F32 => Value::Float(f32::from_le_bytes(b.try_into().unwrap())),
            };
            fields.push((name, value));
            at += kind.size();
        }
        Ok(Self { fields })
    }

    /// A record with every field 0.
    pub fn zeroed(layout: Layout) -> Self {
        Self {
            fields: layout
                .iter()
                .map(|&(n, k)| {
                    (
                        n,
                        if k == F32 {
                            Value::Float(0.0)
                        } else {
                            Value::Int(0)
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Value {
        self.fields
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("no field {name}"))
    }

    pub fn f(&self, name: &str) -> f32 {
        self.get(name).as_f32()
    }

    pub fn u(&self, name: &str) -> u64 {
        self.get(name).as_u64()
    }

    /// Set a field; a float field takes the value as a float, others as an integer.
    pub fn set(&mut self, name: &str, value: f64) {
        let slot = self
            .fields
            .iter_mut()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("no field {name}"));
        slot.1 = match slot.1 {
            Value::Float(_) => Value::Float(value as f32),
            Value::Int(_) => Value::Int(value as u64),
        };
    }

    pub fn to_bytes(&self, layout: Layout) -> Vec<u8> {
        let mut out = Vec::with_capacity(size(layout));
        for (&(_, kind), &(_, value)) in layout.iter().zip(&self.fields) {
            match (kind, value) {
                (U8, v) => out.push(v.as_u64() as u8),
                (U16, v) => out.extend_from_slice(&(v.as_u64() as u16).to_le_bytes()),
                (U32, v) => out.extend_from_slice(&(v.as_u64() as u32).to_le_bytes()),
                (U64, v) => out.extend_from_slice(&v.as_u64().to_le_bytes()),
                (F32, v) => out.extend_from_slice(&v.as_f32().to_le_bytes()),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_sizes_match_the_format() {
        assert_eq!(size(HEADER), 48);
        assert_eq!(size(SETTINGS), 288);
        assert_eq!(size(LAYER_DEF), 88);
        assert_eq!(size(PREVIEW), 32);
    }
}
