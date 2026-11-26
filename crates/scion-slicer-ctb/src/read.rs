//! Reading an encrypted CTB file: its header, settings, layer records and layer images.

use crate::crypto;
use crate::fields::{size, Record, HEADER, LAYER_DEF, PREVIEW, SETTINGS};
use crate::rle;

pub const MAGIC_ENCRYPTED: u32 = 0x12FD_0107;

/// One layer: its record, where it is, and its image data with the encryption and the
/// XOR scrambling undone (still run-length encoded).
#[derive(Debug, Clone)]
pub struct Layer {
    pub def: Record,
    /// Where the record starts in the file.
    pub def_offset: u64,
    /// The pointer table's entry for this layer: offset, page, table size, padding.
    pub pointer: [u32; 4],
    pub data: Vec<u8>,
}

impl Layer {
    pub fn pixels(&self, pixel_count: usize) -> Result<Vec<u8>, String> {
        rle::decode(&self.data, pixel_count)
    }
}

#[derive(Debug, Clone)]
pub struct CtbFile {
    pub header: Record,
    pub settings: Record,
    pub large_preview: Record,
    pub small_preview: Record,
    pub machine_name: String,
    pub layers: Vec<Layer>,
    /// The decrypted signature (32 bytes): the SHA-256 of the settings' checksum value.
    pub signature: Vec<u8>,
    /// Whether the signature is the SHA-256 of the checksum value, as Chitubox writes it.
    pub signature_ok: bool,
    pub len: usize,
}

fn slice(b: &[u8], at: usize, len: usize) -> Result<&[u8], String> {
    b.get(at..at + len)
        .ok_or_else(|| format!("file ends before byte {}", at + len))
}

impl CtbFile {
    pub fn parse(b: &[u8]) -> Result<Self, String> {
        let header = Record::parse(HEADER, b)?;
        if header.u("Magic") as u32 != MAGIC_ENCRYPTED {
            return Err(format!(
                "not an encrypted CTB file (magic {:#x})",
                header.u("Magic")
            ));
        }
        let (so, ss) = (
            header.u("SettingsOffset") as usize,
            header.u("SettingsSize") as usize,
        );
        let mut raw = slice(b, so, ss)?.to_vec();
        crypto::decrypt(&mut raw)?;
        let settings = Record::parse(SETTINGS, &raw)?;

        let preview = |name: &str| {
            Record::parse(PREVIEW, slice(b, settings.u(name) as usize, size(PREVIEW))?)
        };
        let large_preview = preview("LargePreviewOffset")?;
        let small_preview = preview("SmallPreviewOffset")?;
        let machine_name = String::from_utf8_lossy(slice(
            b,
            settings.u("MachineNameOffset") as usize,
            settings.u("MachineNameSize") as usize,
        )?)
        .into_owned();

        let count = settings.u("LayerCount") as usize;
        let pointers = settings.u("LayerPointersOffset") as usize;
        let xor_key = settings.u("LayerXorKey") as u32;
        let mut layers = Vec::with_capacity(count);
        for i in 0..count {
            let p = slice(b, pointers + i * 16, 16)?;
            let pointer =
                [0, 1, 2, 3].map(|k| u32::from_le_bytes(p[k * 4..k * 4 + 4].try_into().unwrap()));
            let def_offset = ((pointer[1] as u64) << 32) | pointer[0] as u64;
            let def = Record::parse(LAYER_DEF, slice(b, def_offset as usize, size(LAYER_DEF))?)?;
            let data_at = ((def.u("PageNumber")) << 32) | def.u("LayerDataOffset");
            let mut data = slice(b, data_at as usize, def.u("LayerDataLength") as usize)?.to_vec();
            let (eo, el) = (
                def.u("EncryptedDataOffset") as usize,
                def.u("EncryptedDataLength") as usize,
            );
            if el > 0 {
                let part = data
                    .get_mut(eo..eo + el)
                    .ok_or(format!("layer {i}: encrypted part outside its data"))?;
                crypto::decrypt(part)?;
            }
            crypto::layer_xor(xor_key, i as u32, &mut data);
            layers.push(Layer {
                def,
                def_offset,
                pointer,
                data,
            });
        }

        let (sig_at, sig_len) = (
            header.u("SignatureOffset") as usize,
            header.u("SignatureSize") as usize,
        );
        let mut signature = slice(b, sig_at, sig_len)?.to_vec();
        crypto::decrypt(&mut signature)?;
        use sha2::Digest;
        let expect = sha2::Sha256::digest(settings.u("ChecksumValue").to_le_bytes());
        let signature_ok = signature == expect.as_slice();

        Ok(Self {
            header,
            settings,
            large_preview,
            small_preview,
            machine_name,
            layers,
            signature,
            signature_ok,
            len: b.len(),
        })
    }

    pub fn pixel_count(&self) -> usize {
        self.settings.u("ResolutionX") as usize * self.settings.u("ResolutionY") as usize
    }
}
