//! A compact binary codec for moving a cooked [`GeometrySet`] across the
//! import-worker boundary.
//!
//! The worker parses a model into a `GeometrySet` in its own wasm heap;
//! [`pack`] serializes the geometry buffers into one little-endian byte
//! blob that transfers (moves) to the main instance, where [`unpack`]
//! reconstructs the set with a single memcpy per buffer (aligned
//! destinations via `bytemuck`, so no per-element loop).
//!
//! Geometry crosses as raw buffers: positions, normals, UVs, indices, the
//! topology tag, the material index, and both attribute-domain lane
//! sections (point, then primitive) per mesh. Materials follow as a
//! serde-JSON header per material (factors, paths, alpha mode) plus raw
//! RGBA blobs for the five texture roles (the `*_texture_data` fields are
//! `serde(skip)` and travel binary), so the full PBR renderer consumes
//! worker-parsed models (the geometry-only form was the original
//! boundary). Bounds are recomputed by [`GeometrySet::from_parts`] on
//! unpack. The format is versionless and same-origin (both sides are the
//! same wasm build), so endianness is fixed little-endian.

use std::sync::Arc;

use bytemuck::{cast_slice, cast_slice_mut};
use solarxy_core::geometry::{MeshTopology, RawImageData, RawMaterialData};
use thiserror::Error;

use crate::set::{AttributeData, AttributeMap, GeometrySet, KernelMesh};

/// A malformed or truncated transfer blob.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransferError {
    #[error("transfer blob truncated (wanted {wanted} more bytes at offset {at})")]
    Truncated { at: usize, wanted: usize },
    #[error("transfer blob material header malformed: {0}")]
    BadMaterial(String),
    #[error("transfer blob attribute lane kind {0} unknown")]
    BadAttributeKind(u32),
}

const HAS_NORMALS: u32 = 1;
const HAS_UVS: u32 = 1 << 1;
const HAS_MATERIAL: u32 = 1 << 2;
/// Bits 3-4 of the mesh flags word carry the topology tag.
const TOPOLOGY_SHIFT: u32 = 3;
const TOPOLOGY_MASK: u32 = 0b11;

fn topology_to_wire(t: MeshTopology) -> u32 {
    match t {
        MeshTopology::Triangles => 0,
        MeshTopology::Lines => 1,
        MeshTopology::Points => 2,
    }
}

fn topology_from_wire(v: u32) -> MeshTopology {
    match v {
        1 => MeshTopology::Lines,
        2 => MeshTopology::Points,
        _ => MeshTopology::Triangles,
    }
}

/// The five texture roles a material can carry, in wire order.
const TEXTURE_ROLES: usize = 5;

fn texture_data_slots(m: &RawMaterialData) -> [&Option<Arc<RawImageData>>; TEXTURE_ROLES] {
    [
        &m.diffuse_texture_data,
        &m.normal_texture_data,
        &m.metallic_roughness_texture_data,
        &m.occlusion_texture_data,
        &m.emissive_texture_data,
    ]
}

fn texture_data_slots_mut(
    m: &mut RawMaterialData,
) -> [&mut Option<Arc<RawImageData>>; TEXTURE_ROLES] {
    [
        &mut m.diffuse_texture_data,
        &mut m.normal_texture_data,
        &mut m.metallic_roughness_texture_data,
        &mut m.occlusion_texture_data,
        &mut m.emissive_texture_data,
    ]
}

/// Serializes a set's geometry and materials into a transfer blob.
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
        flags |= topology_to_wire(mesh.topology) << TOPOLOGY_SHIFT;
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

        push_attributes(&mut out, &mesh.attributes);
        push_attributes(&mut out, &mesh.primitive_attributes);
    }

    // Material section: a serde-JSON header per material (texture-data
    // fields skipped) followed by the raw RGBA blobs for present roles.
    push_u32(&mut out, set.materials.len() as u32);
    for material in &set.materials {
        let header = serde_json::to_vec(material.as_ref()).expect("material header serializes");
        push_u32(&mut out, header.len() as u32);
        out.extend_from_slice(&header);

        let slots = texture_data_slots(material);
        let mut flags = 0u32;
        for (i, slot) in slots.iter().enumerate() {
            if slot.is_some() {
                flags |= 1 << i;
            }
        }
        push_u32(&mut out, flags);
        for slot in slots.iter().filter_map(|s| s.as_deref()) {
            push_u32(&mut out, slot.width);
            push_u32(&mut out, slot.height);
            // The content hash travels with the bytes so the receiving
            // side rebuilds the image without re-walking the pixels.
            out.extend_from_slice(&slot.hash.to_le_bytes());
            push_u32(&mut out, slot.pixels.len() as u32);
            out.extend_from_slice(&slot.pixels);
        }
    }
    out
}

/// Reconstructs a set from a transfer blob (bounds recomputed). Fails on a
/// truncated blob or a malformed material header.
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

        let attributes = read_attributes(&mut r)?;
        let primitive_attributes = read_attributes(&mut r)?;

        meshes.push(KernelMesh {
            name,
            positions,
            normals,
            tex_coords,
            indices,
            material_index,
            topology: topology_from_wire((flags >> TOPOLOGY_SHIFT) & TOPOLOGY_MASK),
            attributes,
            primitive_attributes,
            // The import worker never produces placements, so the
            // transfer blob carries none and its format is unchanged.
            instances: None,
        });
    }

    let material_count = r.u32()? as usize;
    let mut materials = Vec::with_capacity(material_count);
    for _ in 0..material_count {
        let header_len = r.u32()? as usize;
        let header = r.take(header_len)?;
        let mut material: RawMaterialData = serde_json::from_slice(header)
            .map_err(|e| TransferError::BadMaterial(e.to_string()))?;

        let flags = r.u32()?;
        for (i, slot) in texture_data_slots_mut(&mut material)
            .into_iter()
            .enumerate()
        {
            if flags & (1 << i) != 0 {
                let width = r.u32()?;
                let height = r.u32()?;
                let hash = r.u64()?;
                let byte_len = r.u32()? as usize;
                let pixels = r.take(byte_len)?.to_vec();
                *slot = Some(Arc::new(RawImageData::from_parts(
                    pixels, width, height, hash,
                )));
            }
        }
        materials.push(Arc::new(material));
    }

    Ok(GeometrySet::from_parts(meshes, materials))
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// One attribute-domain section: lane count, then per lane the name, a
/// kind tag (0 Float, 1 Vec2, 2 Vec3, 3 Vec4), the element count, and the
/// raw little-endian floats.
fn push_attributes(out: &mut Vec<u8>, map: &AttributeMap) {
    push_u32(out, map.len() as u32);
    for (name, data) in map {
        let bytes = name.as_bytes();
        push_u32(out, bytes.len() as u32);
        out.extend_from_slice(bytes);
        let kind = match data {
            AttributeData::Float(_) => 0,
            AttributeData::Vec2(_) => 1,
            AttributeData::Vec3(_) => 2,
            AttributeData::Vec4(_) => 3,
        };
        push_u32(out, kind);
        push_u32(out, data.len() as u32);
        match data {
            AttributeData::Float(v) => out.extend_from_slice(cast_slice::<f32, u8>(v)),
            AttributeData::Vec2(v) => out.extend_from_slice(cast_slice::<[f32; 2], u8>(v)),
            AttributeData::Vec3(v) => out.extend_from_slice(cast_slice::<[f32; 3], u8>(v)),
            AttributeData::Vec4(v) => out.extend_from_slice(cast_slice::<[f32; 4], u8>(v)),
        }
    }
}

fn read_attributes(r: &mut Reader) -> Result<AttributeMap, TransferError> {
    let count = r.u32()? as usize;
    let mut map = AttributeMap::new();
    for _ in 0..count {
        let name_len = r.u32()? as usize;
        let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
        let kind = r.u32()?;
        let len = r.u32()? as usize;
        let data = match kind {
            0 => AttributeData::Float(Arc::new(r.f32_vec(len)?)),
            1 => AttributeData::Vec2(Arc::new(r.vec2(len)?)),
            2 => AttributeData::Vec3(Arc::new(r.vec3(len)?)),
            3 => AttributeData::Vec4(Arc::new(r.vec4(len)?)),
            other => return Err(TransferError::BadAttributeKind(other)),
        };
        map.insert(name, data);
    }
    Ok(map)
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

    fn u64(&mut self) -> Result<u64, TransferError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
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

    fn f32_vec(&mut self, count: usize) -> Result<Vec<f32>, TransferError> {
        let src = self.take(count * 4)?;
        let mut v = vec![0.0f32; count];
        cast_slice_mut::<f32, u8>(&mut v).copy_from_slice(src);
        Ok(v)
    }

    fn vec4(&mut self, count: usize) -> Result<Vec<[f32; 4]>, TransferError> {
        let src = self.take(count * 16)?;
        let mut v = vec![[0.0f32; 4]; count];
        cast_slice_mut::<[f32; 4], u8>(&mut v).copy_from_slice(src);
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
            topology: solarxy_core::geometry::MeshTopology::Triangles,
            attributes: AttributeMap::new(),
            primitive_attributes: AttributeMap::new(),
            instances: None,
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
    fn topology_tags_and_attribute_lanes_round_trip() {
        let mut cloud = KernelMesh::points("cloud", vec![[0.0; 3], [1.0, 2.0, 3.0]]);
        cloud.attributes.insert(
            crate::set::reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::new(vec![[1.0, 0.5, 0.25, 1.0], [0.0, 1.0, 0.0, 0.5]])),
        );
        cloud.attributes.insert(
            "weight".to_string(),
            AttributeData::Float(Arc::new(vec![0.1, 0.9])),
        );
        cloud.primitive_attributes.insert(
            "tag".to_string(),
            AttributeData::Vec3(Arc::new(vec![[7.0, 8.0, 9.0], [1.0, 1.0, 1.0]])),
        );
        let wire = KernelMesh::polyline("wire", vec![[0.0; 3], [4.0, 0.0, 0.0]], vec![0, 1]);
        let set = GeometrySet::from_parts(vec![cloud, wire], Vec::new());

        let back = unpack(&pack(&set)).expect("round trip");
        assert_eq!(back.meshes[0].topology, MeshTopology::Points);
        assert_eq!(back.meshes[1].topology, MeshTopology::Lines);
        assert_eq!(
            back.meshes[0].attributes.get(crate::set::reserved::COLOR),
            Some(&AttributeData::Vec4(Arc::new(vec![
                [1.0, 0.5, 0.25, 1.0],
                [0.0, 1.0, 0.0, 0.5]
            ])))
        );
        assert_eq!(
            back.meshes[0].attributes.get("weight"),
            Some(&AttributeData::Float(Arc::new(vec![0.1, 0.9])))
        );
        assert_eq!(
            back.meshes[0].primitive_attributes.get("tag"),
            Some(&AttributeData::Vec3(Arc::new(vec![
                [7.0, 8.0, 9.0],
                [1.0, 1.0, 1.0]
            ])))
        );
        assert!(back.meshes[1].attributes.is_empty());
        assert!(
            !back.is_renderable_empty(),
            "the unpacked cloud stays renderable"
        );
    }

    #[test]
    fn truncated_blob_errors() {
        assert!(matches!(
            unpack(&[0, 0]),
            Err(TransferError::Truncated { .. })
        ));
    }

    #[test]
    fn materials_round_trip_with_texture_bytes() {
        let mesh = KernelMesh::new(
            "textured",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let material = RawMaterialData {
            name: "steel".to_string(),
            roughness_factor: 0.4,
            metallic_factor: 0.9,
            emissive_factor: [0.1, 0.2, 0.3],
            alpha_cutoff: 0.5,
            diffuse: Some([0.8, 0.7, 0.6]),
            diffuse_texture_data: Some(Arc::new(RawImageData::new(
                vec![1, 2, 3, 4, 5, 6, 7, 8],
                2,
                1,
            ))),
            emissive_texture_data: Some(Arc::new(RawImageData::new(vec![9, 9, 9, 9], 1, 1))),
            ..RawMaterialData::default()
        };
        let set = GeometrySet::from_parts(vec![mesh], vec![Arc::new(material)]);

        let back = unpack(&pack(&set)).expect("round trip");
        assert_eq!(back.materials.len(), 1);
        let m = &back.materials[0];
        assert_eq!(m.name, "steel");
        assert!((m.roughness_factor - 0.4).abs() < 1e-6);
        assert!((m.metallic_factor - 0.9).abs() < 1e-6);
        assert_eq!(m.diffuse, Some([0.8, 0.7, 0.6]));
        let tex = m.diffuse_texture_data.as_ref().expect("diffuse texture");
        assert_eq!(tex.pixels, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!((tex.width, tex.height), (2, 1));
        assert_eq!(
            tex.hash,
            RawImageData::content_hash(&tex.pixels, 2, 1),
            "content hash travels through the codec intact"
        );
        let em = m.emissive_texture_data.as_ref().expect("emissive texture");
        assert_eq!(em.pixels, vec![9, 9, 9, 9]);
        assert!(m.normal_texture_data.is_none());
    }

    #[test]
    fn malformed_material_header_errors() {
        let set = GeometrySet::from_parts(vec![], vec![]);
        let mut blob = pack(&set);
        // Rewrite the material count to 1 and append a bogus header.
        let count_at = blob.len() - 4;
        blob[count_at..].copy_from_slice(&1u32.to_le_bytes());
        blob.extend_from_slice(&3u32.to_le_bytes());
        blob.extend_from_slice(b"{!}");
        assert!(matches!(unpack(&blob), Err(TransferError::BadMaterial(_))));
    }
}
