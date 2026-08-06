//! What the inspection panels read about an open engine scene.
//!
//! A file-loaded model hands the panels one [`Model`], one [`ModelStats`]
//! and one [`ValidationReport`]. An engine scene has N objects, each with
//! its own model, its own counts and its own report, so something has to
//! collapse them into the shapes the panels already take. That is this
//! module.
//!
//! It is rebuilt when a scene delta is drained rather than per frame:
//! deltas are the only thing that changes any of it, and the merge order
//! has to be stable anyway (see [`MergedValidation`]).
//!
//! [`Model`]: solarxy_renderer::model::Model
//! [`ModelStats`]: solarxy_renderer::resources::ModelStats

use solarxy_core::MeshTopology;
use solarxy_core::scene::SceneObjectId;
use solarxy_core::validation::ValidationReport;
use solarxy_renderer::scene_objects::SceneObjects;

/// Everything the Properties and Outliner panels need about the open
/// scene. `None` on `State` whenever no scene file is open.
pub(crate) struct EngineSceneInfo {
    /// The `.slxy` this scene was opened from.
    pub filename: String,
    pub path: String,
    pub file_size: u64,
    /// Display name per object, in `SceneObjects` iteration order.
    pub object_names: Vec<(SceneObjectId, String)>,
    /// Geometry counters summed across every object.
    pub counts: SceneGeometryCounts,
    /// Every object's issues merged into the single report the Properties
    /// panel already renders.
    pub validation: MergedValidation,
}

impl EngineSceneInfo {
    /// A scene that has been opened but has not cooked anything yet.
    pub fn new(filename: String, path: String, file_size: u64) -> Self {
        Self {
            filename,
            path,
            file_size,
            object_names: Vec::new(),
            counts: SceneGeometryCounts::default(),
            validation: MergedValidation::default(),
        }
    }
}

/// Geometry counters for a whole scene.
///
/// Instanced geometry is counted **as drawn**: a scatter's prototype is one
/// mesh in memory but many triangles on screen, and a panel that reported
/// only the prototype would describe a 10,000-copy scatter as a single
/// small mesh. Both numbers are kept, because the source counts are what
/// explain the drawn ones.
#[derive(Default, Clone, Copy)]
pub(crate) struct SceneGeometryCounts {
    pub objects: usize,
    pub meshes: usize,
    pub materials: usize,
    /// Placements across every mesh. Equal to `meshes` when nothing is
    /// instanced, since the uninstanced case is one identity placement.
    pub instances: usize,
    /// Counts multiplied by each mesh's placements: what the frame draws.
    pub drawn_tris: usize,
    pub drawn_verts: usize,
    /// Counts before placements: what the scene holds once.
    pub unique_tris: usize,
    pub unique_verts: usize,
    pub has_uvs: bool,
}

impl SceneGeometryCounts {
    /// Whether any mesh is placed more than once, which is the only case
    /// where the drawn and source counts differ and the split is worth
    /// showing.
    pub fn is_instanced(&self) -> bool {
        self.drawn_tris != self.unique_tris || self.drawn_verts != self.unique_verts
    }
}

/// Sum every object's geometry into one set of counters.
///
/// Hidden objects still count, matching the file-model path, where a mesh
/// hidden through the Outliner stays in the model's stats. The panel
/// describes the scene, not the frame.
pub(crate) fn count_geometry(objects: &SceneObjects) -> SceneGeometryCounts {
    let mut c = SceneGeometryCounts {
        objects: objects.len(),
        ..SceneGeometryCounts::default()
    };
    for (_, object) in objects.iter() {
        c.meshes += object.model.meshes.len();
        c.materials += object.model.materials.len();
        c.has_uvs |= object.model.has_uvs;
        for mesh in &object.model.meshes {
            let placements = mesh.instance_count as usize;
            c.instances += placements;
            // Only triangle meshes have a triangle count; a line or point
            // mesh's index buffer is not triples, and dividing it by three
            // would invent geometry that does not exist.
            let tris = if mesh.topology == MeshTopology::Triangles {
                mesh.num_elements as usize / 3
            } else {
                0
            };
            let verts = mesh.num_vertices as usize;
            c.unique_tris += tris;
            c.unique_verts += verts;
            c.drawn_tris += tris * placements;
            c.drawn_verts += verts * placements;
        }
    }
    c
}

/// One report built from many, with the provenance the fly-to needs.
///
/// The panel addresses a clicked row by its index into the merged
/// `issues`, so resolving it back to geometry needs the object it came
/// from and its index within that object's own report. Deriving that a
/// second time inside the click handler would mean two orderings that must
/// agree forever; keeping it beside the merge means there is only one.
#[derive(Default)]
pub(crate) struct MergedValidation {
    pub report: ValidationReport,
    /// Parallel to `report.issues`: owning object, and the issue's index in
    /// that object's report.
    pub owners: Vec<(SceneObjectId, usize)>,
    /// Parallel to `report.issues`: the owning object's display name.
    ///
    /// Carried rather than looked up at draw time because an issue scope
    /// renders as `Mesh [0]` for every object, so a merged list without
    /// the owner shows several identical rows for different geometry.
    pub labels: Vec<String>,
}

/// Merge per-object reports, preserving the order they arrive in.
///
/// Free-standing and taking plain borrows rather than reaching into
/// `SceneObjects`, so the ordering contract above can be tested without a
/// GPU.
pub(crate) fn merge_validation<'a>(
    per_object: impl IntoIterator<Item = (SceneObjectId, &'a str, &'a ValidationReport)>,
) -> MergedValidation {
    let mut merged = MergedValidation::default();
    for (id, name, report) in per_object {
        for (local, issue) in report.issues.iter().enumerate() {
            merged.report.issues.push(issue.clone());
            merged.owners.push((id, local));
            merged.labels.push(name.to_string());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::validation::{IssueKind, IssueScope, Severity, ValidationIssue};

    fn issue(scope: IssueScope, message: &str) -> ValidationIssue {
        ValidationIssue {
            severity: Severity::Warning,
            scope,
            kind: IssueKind::NonManifoldEdge,
            message: message.to_string(),
        }
    }

    fn report(issues: Vec<ValidationIssue>) -> ValidationReport {
        ValidationReport { issues }
    }

    /// The merge's whole purpose: a row index in the merged report must
    /// resolve to the object it came from and its index inside that
    /// object's own report. Getting this wrong flies the camera to another
    /// object's mesh, which looks entirely plausible on screen and is why
    /// this is asserted rather than eyeballed.
    #[test]
    fn merged_indices_resolve_to_their_owning_object() {
        let a = report(vec![
            issue(IssueScope::Mesh(0), "a0"),
            issue(IssueScope::Mesh(1), "a1"),
        ]);
        let b = report(vec![issue(IssueScope::Mesh(0), "b0")]);
        let c = report(vec![
            issue(IssueScope::Model, "c0"),
            issue(IssueScope::Mesh(7), "c1"),
        ]);

        let merged = merge_validation([
            (SceneObjectId(10), "sphere1", &a),
            (SceneObjectId(20), "grid1", &b),
            (SceneObjectId(30), "scatter1", &c),
        ]);

        assert_eq!(merged.report.issues.len(), 5);
        assert_eq!(merged.owners.len(), 5);
        assert_eq!(merged.labels.len(), 5);

        let resolved: Vec<_> = merged
            .report
            .issues
            .iter()
            .zip(&merged.owners)
            .zip(&merged.labels)
            .map(|((issue, (id, local)), label)| {
                (issue.message.as_str(), id.0, *local, label.as_str())
            })
            .collect();

        assert_eq!(
            resolved,
            vec![
                ("a0", 10, 0, "sphere1"),
                ("a1", 10, 1, "sphere1"),
                ("b0", 20, 0, "grid1"),
                ("c0", 30, 0, "scatter1"),
                ("c1", 30, 1, "scatter1"),
            ]
        );
    }

    /// The scopes deliberately collide across objects: two objects each
    /// report `Mesh [0]`, and only the owner distinguishes them. This is
    /// the case the owner index exists for.
    #[test]
    fn colliding_scopes_stay_distinguishable_by_owner() {
        let a = report(vec![issue(IssueScope::Mesh(0), "same scope")]);
        let b = report(vec![issue(IssueScope::Mesh(0), "same scope")]);

        let merged = merge_validation([
            (SceneObjectId(1), "first", &a),
            (SceneObjectId(2), "second", &b),
        ]);

        // `IssueScope` is not comparable, and is deliberately not made so
        // for a test; what matters is that both rows *render* identically,
        // which is exactly the ambiguity the owner resolves.
        assert_eq!(
            merged.report.issues[0].scope.to_string(),
            merged.report.issues[1].scope.to_string()
        );
        assert_eq!(merged.owners[0], (SceneObjectId(1), 0));
        assert_eq!(merged.owners[1], (SceneObjectId(2), 0));
        assert_eq!(merged.labels, vec!["first", "second"]);
    }

    /// An object with a clean report contributes nothing and must not
    /// shift the objects after it.
    #[test]
    fn clean_objects_do_not_shift_later_indices() {
        let clean = report(vec![]);
        let dirty = report(vec![issue(IssueScope::Mesh(2), "only issue")]);

        let merged = merge_validation([
            (SceneObjectId(1), "clean", &clean),
            (SceneObjectId(2), "dirty", &dirty),
        ]);

        assert_eq!(merged.report.issues.len(), 1);
        assert_eq!(merged.owners, vec![(SceneObjectId(2), 0)]);
        assert_eq!(merged.labels, vec!["dirty"]);
    }

    #[test]
    fn merging_nothing_yields_a_clean_report() {
        let merged = merge_validation([]);
        assert!(merged.report.is_clean());
        assert!(merged.owners.is_empty());
        assert!(merged.labels.is_empty());
    }
}
