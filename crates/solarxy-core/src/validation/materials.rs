//! Material-reference checks: each mesh's `material_index` must point into
//! the model's material list. (Texture-existence checks live alongside the
//! `MissingTexture` issue kind; they're triggered from the loader side and
//! routed through this module's report types.)

use super::types::{IssueKind, IssueScope, Severity, ValidationIssue};
use crate::geometry::RawMeshData;

/// Material-index range check. Returns an error issue if the mesh
/// references a material slot that doesn't exist.
pub(super) fn check_material_ref(
    mesh_index: usize,
    mesh: &RawMeshData,
    materials_len: usize,
) -> Option<ValidationIssue> {
    if let Some(mat_id) = mesh.material_index
        && mat_id >= materials_len
    {
        return Some(ValidationIssue {
            severity: Severity::Error,
            scope: IssueScope::Mesh(mesh_index),
            kind: IssueKind::InvalidMaterialRef,
            message: format!(
                "Material ID {} is out of range (only {} materials available)",
                mat_id, materials_len
            ),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::single_triangle_raw;
    use super::super::validate_raw_model;
    use super::*;
    use crate::geometry::{AlphaMode, RawMaterialData};

    #[test]
    fn invalid_material_ref() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].material_index = Some(5);
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::InvalidMaterialRef)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn invalid_material_ref_at_boundary() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].material_index = Some(1);
        raw.materials.push(RawMaterialData {
            name: "mat0".to_string(),
            diffuse_texture_path: None,
            normal_texture_path: None,
            diffuse_texture_data: None,
            normal_texture_data: None,
            metallic_roughness_texture_path: None,
            metallic_roughness_texture_data: None,
            occlusion_texture_path: None,
            occlusion_texture_data: None,
            emissive_texture_path: None,
            emissive_texture_data: None,
            roughness_factor: 0.5,
            metallic_factor: 0.0,
            emissive_factor: [0.0; 3],
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            ambient: None,
            diffuse: None,
            specular: None,
            shininess: None,
            dissolve: None,
            optical_density: None,
            ambient_texture_name: None,
            diffuse_texture_name: None,
            specular_texture_name: None,
            normal_texture_name: None,
            shininess_texture_name: None,
            dissolve_texture_name: None,
        });

        let result = validate_raw_model(&raw, "obj");
        let invalid: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::InvalidMaterialRef)
            .collect();
        assert_eq!(invalid.len(), 1);

        raw.meshes[0].material_index = Some(0);
        let result = validate_raw_model(&raw, "obj");
        let invalid: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::InvalidMaterialRef)
            .collect();
        assert!(invalid.is_empty());
    }
}
