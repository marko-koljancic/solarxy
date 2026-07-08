//! A compact binary codec for moving a cooked [`GeometrySet`] across the
//! import-worker boundary.
//!
//! The worker parses a model into a `GeometrySet` in its own wasm heap;
//! [`pack`] serializes the geometry buffers into one little-endian byte
//! blob that transfers (moves) to the main instance, where [`unpack`]
//! reconstructs the set with a single memcpy per buffer (aligned
//! destinations via `bytemuck`, so no per-element loop).
//!
//! Only geometry crosses: positions, normals, UVs, indices, and the
//! material index per mesh. Materials are dropped, because the web forward
//! renderer does not consume them yet (a documented Phase-5 MVP boundary);
//! bounds are recomputed by [`GeometrySet::from_parts`] on unpack. The
//! format is versionless and same-origin (both sides are the same wasm
//! build), so endianness is fixed little-endian.

use std::sync::Arc;

use bytemuck::{cast_slice, cast_slice_mut};
use thiserror::Error;

use crate::set::{AttributeMap, GeometrySet, KernelMesh};

/// A malformed or truncated transfer blob.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransferError {
    #[error("transfer blob truncated (wanted {wanted} more bytes at offset {at})")]
    Truncated { at: usize, wanted: usize },
}

const HAS_NORMALS: u32 = 1;
const HAS_UVS: u32 = 1 << 1;
const HAS_MATERIAL: u32 = 1 << 2;

/// Serializes a set's geometry into a transfer blob (materials dropped).
#[must_use]
pub fn pack(set: &GeometrySet) -> Vec<u8> {
    let mut out = Vec::new();
    push_u32(&mut out, set.meshes.len() as u32);
    for mesh in &set.meshes {
        let name = mesh.name.as_bytes();
        push_u32(&mut out, name.len() as u32);
        out.extend_from_slice(name);

        push_u32(&mut out, mesh.positions.len() as u32);
        let mut flags = 0;
        if mesh.normals.is_some() {
            flags |= HAS_NORMALS;
        }
        if mesh.tex_coords.is_some() {
            flags |= HAS_UVS;
        }
        if mesh.material_index.is_some() {
            flags |= HAS_MATERIAL;
        }
        push_u32(&mut out, flags);

        out.extend_from_slice(cast_slice::<[f32; 3], u8>(&mesh.positions));
        if let Some(normals) = &mesh.normals {
            out.extend_from_slice(cast_slice::<[f32; 3], u8>(normals));
        }
        if let Some(uvs) = &mesh.tex_coords {
            out.extend_from_slice(cast_slice::<[f32; 2], u8>(uvs));
        }

        push_u32(&mut out, mesh.indices.len() as u32);
        out.extend_from_slice(cast_slice::<u32, u8>(&mesh.indices));

        if let Some(mi) = mesh.material_index {
            push_u32(&mut out, mi as u32);
        }
    }
    out
}

/// Reconstructs a set from a transfer blob (bounds recomputed, no
/// materials). Fails only on a truncated blob.
pub fn unpack(bytes: &[u8]) -> Result<GeometrySet, TransferError> {
    let mut r = Reader { bytes, pos: 0 };
    let mesh_count = r.u32()? as usize;
    let mut meshes = Vec::with_capacity(mesh_count);
    for _ in 0..mesh_count {
        let name_len = r.u32()? as usize;
        let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();

        let vcount = r.u32()? as usize;
        let flags = r.u32()?;

        let positions = Arc::new(r.vec3(vcount)?);
        let normals = if flags & HAS_NORMALS != 0 {
            Some(Arc::new(r.vec3(vcount)?))
        } else {
            None
        };
        let tex_coords = if flags & HAS_UVS != 0 {
            Some(Arc::new(r.vec2(vcount)?))
        } else {
            None
        };

        let icount = r.u32()? as usize;
        let indices = Arc::new(r.u32_vec(icount)?);

        let material_index = if flags & HAS_MATERIAL != 0 {
            Some(r.u32()? as usize)
        } else {
            None
        };

        meshes.push(KernelMesh {
            name,
            positions,
            normals,
            tex_coords,
            indices,
            material_index,
            attributes: AttributeMap::new(),
        });
    }
    Ok(GeometrySet::from_parts(meshes, Vec::new()))
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], TransferError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.bytes.len());
        let Some(end) = end else {
            return Err(TransferError::Truncated {
                at: self.pos,
                wanted: n,
            });
        };
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, TransferError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn vec3(&mut self, count: usize) -> Result<Vec<[f32; 3]>, TransferError> {
        let src = self.take(count * 12)?;
        let mut v = vec![[0.0f32; 3]; count];
        cast_slice_mut::<[f32; 3], u8>(&mut v).copy_from_slice(src);
        Ok(v)
    }

    fn vec2(&mut self, count: usize) -> Result<Vec<[f32; 2]>, TransferError> {
        let src = self.take(count * 8)?;
        let mut v = vec![[0.0f32; 2]; count];
        cast_slice_mut::<[f32; 2], u8>(&mut v).copy_from_slice(src);
        Ok(v)
    }

    fn u32_vec(&mut self, count: usize) -> Result<Vec<u32>, TransferError> {
        let src = self.take(count * 4)?;
        let mut v = vec![0u32; count];
        cast_slice_mut::<u32, u8>(&mut v).copy_from_slice(src);
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_geometry_buffers() {
        let mesh = KernelMesh {
            name: "imported".to_string(),
            positions: Arc::new(vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]),
            normals: Some(Arc::new(vec![[0.0, 0.0, 1.0]; 3])),
            tex_coords: Some(Arc::new(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])),
            indices: Arc::new(vec![0, 1, 2]),
            material_index: Some(2),
            attributes: AttributeMap::new(),
        };
        let set = GeometrySet::from_parts(vec![mesh], Vec::new());

        let blob = pack(&set);
        let back = unpack(&blob).expect("round trip");

        assert_eq!(back.meshes.len(), 1);
        let m = &back.meshes[0];
        assert_eq!(m.name, "imported");
        assert_eq!(
            *m.positions,
            vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]
        );
        assert_eq!(m.normals.as_deref(), Some(&vec![[0.0, 0.0, 1.0]; 3]));
        assert_eq!(
            m.tex_coords.as_deref(),
            Some(&vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])
        );
        assert_eq!(*m.indices, vec![0, 1, 2]);
        assert_eq!(m.material_index, Some(2));
        // Bounds were recomputed on unpack.
        assert!((back.bounds.max.x - 4.0).abs() < 1e-6);
    }

    #[test]
    fn no_optional_buffers_round_trips() {
        let mesh = KernelMesh::new(
            "bare",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let set = GeometrySet::from_mesh(mesh);
        let back = unpack(&pack(&set)).unwrap();
        assert!(back.meshes[0].normals.is_none());
        assert!(back.meshes[0].tex_coords.is_none());
        assert_eq!(back.meshes[0].material_index, None);
    }

    #[test]
    fn truncated_blob_errors() {
        assert!(matches!(
            unpack(&[0, 0]),
            Err(TransferError::Truncated { .. })
        ));
    }
}
