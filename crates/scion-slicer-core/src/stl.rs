//! Loading binary STL files into the engine's triangle type.

use std::path::Path;

use dragonfruit_slicing_engine::geometry::{parse_triangles, Triangle};

/// A loaded mesh and its bounding box in mm.
pub struct Mesh {
    pub triangles: Vec<Triangle>,
    pub bbox_min: [f64; 3],
    pub bbox_max: [f64; 3],
}

/// Load a binary STL. ASCII STL is not supported yet.
pub fn load(path: &Path) -> Result<Mesh, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let flat = parse_binary(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bbox_min = [f64::INFINITY; 3];
    let mut bbox_max = [f64::NEG_INFINITY; 3];
    for v in flat.chunks_exact(3) {
        for i in 0..3 {
            bbox_min[i] = bbox_min[i].min(v[i] as f64);
            bbox_max[i] = bbox_max[i].max(v[i] as f64);
        }
    }
    Ok(Mesh {
        triangles: parse_triangles(&flat),
        bbox_min,
        bbox_max,
    })
}

/// Vertex positions of a binary STL as a flat list (x, y, z of each corner of each
/// triangle in turn).
pub fn parse_binary(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() < 84 {
        return Err("too short to be a binary STL".into());
    }
    let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let expected = 84 + count * 50;
    if bytes.len() < expected {
        if bytes.starts_with(b"solid") {
            return Err("ASCII STL is not supported yet; export it as binary".into());
        }
        return Err(format!("header says {count} triangles but the file holds fewer"));
    }
    if count == 0 {
        return Err("the mesh has no triangles".into());
    }
    let mut flat = Vec::with_capacity(count * 9);
    for t in 0..count {
        let o = 84 + t * 50 + 12; // skip the facet normal
        for k in 0..9 {
            let at = o + k * 4;
            flat.push(f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]));
        }
    }
    Ok(flat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_and_ascii_files() {
        assert!(parse_binary(b"solid x").is_err());
        let mut ascii = b"solid cube\n".to_vec();
        ascii.resize(84, b' ');
        ascii[80..84].copy_from_slice(&5u32.to_le_bytes());
        assert!(parse_binary(&ascii).unwrap_err().contains("ASCII"));
    }

    #[test]
    fn reads_one_triangle() {
        let mut b = vec![0u8; 84];
        b[80..84].copy_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&[0u8; 12]);
        for v in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[0, 0]);
        assert_eq!(parse_binary(&b).unwrap(), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    }
}
