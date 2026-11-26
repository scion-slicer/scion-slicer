//! The fixed AES-256-CBC key and IV that encrypted CTB files use for their settings block,
//! their signature and the encrypted part of a layer, and the XOR stream that scrambles
//! each layer's image data.
//!
//! The key and IV are the publicly known ones UVtools and DragonFruit use. They are kept
//! XOR-obfuscated here exactly as DragonFruit's MIT-licensed CTB plugin keeps them
//! (`plugins/ctb/slicing/rust/ctb_crypto.rs`), so the constants match its source.

use aes::cipher::block_padding::NoPadding;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use aes::Aes256;

const OBFUSCATION: &[u8; 14] = b"DragonFruitFTW";
const KEY_XOR: [u8; 32] = [
    0x94, 0x29, 0xEF, 0x54, 0x1E, 0xB0, 0x7B, 0x68, 0x90, 0x26, 0x56, 0x9B, 0x8B, 0x0C, 0xB9, 0xE6,
    0xCA, 0x3A, 0x0B, 0x54, 0xDB, 0x0C, 0xCA, 0xC6, 0x36, 0x45, 0xA7, 0x47, 0x9C, 0x20, 0x4B, 0x8D,
];
const IV_XOR: [u8; 16] = [
    0x4B, 0x73, 0x6B, 0x62, 0x6A, 0x65, 0x40, 0x75, 0x7D, 0x6F, 0x7E, 0x4A, 0x58, 0x5A, 0x4D, 0x7D,
];

fn deobfuscate<const N: usize>(input: [u8; N]) -> [u8; N] {
    let mut out = input;
    for (i, b) in out.iter_mut().enumerate() {
        *b ^= OBFUSCATION[i % OBFUSCATION.len()];
    }
    out
}

fn key_iv() -> ([u8; 32], [u8; 16]) {
    (deobfuscate(KEY_XOR), deobfuscate(IV_XOR))
}

/// Encrypt in place. The length must be a multiple of 16.
pub fn encrypt(bytes: &mut [u8]) -> Result<(), String> {
    if !bytes.len().is_multiple_of(16) {
        return Err(format!(
            "AES block of {} bytes is not a multiple of 16",
            bytes.len()
        ));
    }
    let (key, iv) = key_iv();
    let len = bytes.len();
    cbc::Encryptor::<Aes256>::new(&key.into(), &iv.into())
        .encrypt_padded_mut::<NoPadding>(bytes, len)
        .map(|_| ())
        .map_err(|e| format!("AES encryption failed: {e}"))
}

/// Decrypt in place. The length must be a multiple of 16.
pub fn decrypt(bytes: &mut [u8]) -> Result<(), String> {
    if !bytes.len().is_multiple_of(16) {
        return Err(format!(
            "AES block of {} bytes is not a multiple of 16",
            bytes.len()
        ));
    }
    let (key, iv) = key_iv();
    cbc::Decryptor::<Aes256>::new(&key.into(), &iv.into())
        .decrypt_padded_mut::<NoPadding>(bytes)
        .map(|_| ())
        .map_err(|e| format!("AES decryption failed: {e}"))
}

/// Scramble or unscramble a layer's image data with the file's layer XOR key (the same
/// operation both ways). A key of 0 means the data isn't scrambled.
pub fn layer_xor(seed: u32, layer_index: u32, bytes: &mut [u8]) {
    if seed == 0 {
        return;
    }
    let init = seed.wrapping_mul(0x2d83_cdac).wrapping_add(0xd8a8_3423);
    let mut key = layer_index
        .wrapping_mul(0x1e15_30cd)
        .wrapping_add(0xec3d_47cd)
        .wrapping_mul(init);
    for chunk in bytes.chunks_mut(4) {
        for (b, k) in chunk.iter_mut().zip(key.to_le_bytes()) {
            *b ^= k;
        }
        key = key.wrapping_add(init);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_round_trips() {
        let mut block = *b"0123456789abcdef0123456789abcdef";
        encrypt(&mut block).unwrap();
        assert_ne!(&block, b"0123456789abcdef0123456789abcdef");
        decrypt(&mut block).unwrap();
        assert_eq!(&block, b"0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn xor_is_its_own_inverse() {
        let mut data: Vec<u8> = (0..23).collect();
        layer_xor(435_717_784, 7, &mut data);
        assert_ne!(data, (0..23).collect::<Vec<u8>>());
        layer_xor(435_717_784, 7, &mut data);
        assert_eq!(data, (0..23).collect::<Vec<u8>>());
    }
}
