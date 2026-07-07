//! Debug-build-only developer harness for the multi-object scene: a key
//! toggle (F9) that inserts two cubes with independent transforms through
//! the real `SceneDelta` path, proving the phase-2 exit criterion ("a
//! hidden dev command renders two objects with independent transforms")
//! without any engine.

use std::sync::Arc;

use solarxy_core::scene::{CookedGeometry, CookedMesh, SceneDelta, SceneObjectId, SceneOp};

use super::State;
use crate::gui::ToastSeverity;

const DEV_CUBE_A: SceneObjectId = SceneObjectId(0xDE10);
const DEV_CUBE_B: SceneObjectId = SceneObjectId(0xDE11);

impl State {
    /// Toggle the two dev cubes. Requires a loaded model (the cubes are
    /// placed and sized relative to its bounds, and the scene-level light
    /// and shadow state live on the `ModelScene`).
    pub(super) fn toggle_dev_objects(&mut self) {
        let Some(scene) = &self.scene else {
            self.gui
                .set_toast("Dev objects need a loaded model", ToastSeverity::Warning);
            return;
        };

        if self.scene_objects.get(DEV_CUBE_A).is_some() {
            self.pending_scene_deltas.push(SceneDelta {
                ops: vec![
                    SceneOp::Remove { id: DEV_CUBE_A },
                    SceneOp::Remove { id: DEV_CUBE_B },
                ],
            });
            self.gui
                .set_toast("Dev objects removed", ToastSeverity::Success);
            return;
        }

        let bounds = scene.model.bounds;
        let center = bounds.center();
        let d = bounds.diagonal();
        let s_a = d * 0.12;
        let s_b = d * 0.07;

        let place = |s: f32, dx: f32, dy: f32| -> [[f32; 4]; 4] {
            [
                [s, 0.0, 0.0, 0.0],
                [0.0, s, 0.0, 0.0],
                [0.0, 0.0, s, 0.0],
                [center.x + dx, center.y + dy, center.z, 1.0],
            ]
        };

        self.pending_scene_deltas.push(SceneDelta {
            ops: vec![
                SceneOp::UpsertGeometry {
                    id: DEV_CUBE_A,
                    geometry: Arc::new(dev_cube("dev_cube_a")),
                },
                SceneOp::SetTransform {
                    id: DEV_CUBE_A,
                    transform: place(s_a, -d * 0.45, d * 0.15),
                },
                SceneOp::UpsertGeometry {
                    id: DEV_CUBE_B,
                    geometry: Arc::new(dev_cube("dev_cube_b")),
                },
                SceneOp::SetTransform {
                    id: DEV_CUBE_B,
                    transform: place(s_b, d * 0.45, d * 0.3),
                },
            ],
        });
        self.gui.set_toast(
            "Dev objects: two cubes with independent transforms",
            ToastSeverity::Success,
        );
    }
}

/// A unit cube (side 1, centered at the origin) with 24 vertices so each
/// face carries its own flat normal.
fn dev_cube(name: &str) -> CookedGeometry {
    // (face normal, four corners CCW seen from outside)
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
    ];

    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corners) in &FACES {
        let base = positions.len() as u32;
        for corner in corners {
            positions.push(*corner);
            normals.push(*normal);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let bounds = solarxy_core::geometry::compute_bounds(&positions);
    CookedGeometry {
        meshes: vec![CookedMesh {
            name: name.to_string(),
            positions: Arc::new(positions),
            normals: Some(Arc::new(normals)),
            tex_coords: None,
            indices: Arc::new(indices),
            material_index: None,
        }],
        materials: Vec::new(),
        bounds,
    }
}
