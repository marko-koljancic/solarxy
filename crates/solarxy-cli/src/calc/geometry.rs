use cgmath::Point3;
use solarxy_core::AABB;

use super::analyze::AnalyzerMesh;
use solarxy_core::report::BoundsSummary;

pub fn compute_bounds(meshes: &[AnalyzerMesh]) -> Option<BoundsSummary> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut has_any = false;

    for mesh in meshes {
        for chunk in mesh.positions.chunks_exact(3) {
            has_any = true;
            for i in 0..3 {
                min[i] = min[i].min(chunk[i]);
                max[i] = max[i].max(chunk[i]);
            }
        }
    }

    if !has_any {
        return None;
    }

    let aabb = AABB {
        min: Point3::new(min[0], min[1], min[2]),
        max: Point3::new(max[0], max[1], max[2]),
    };
    let size = aabb.size();
    let center = aabb.center();

    Some(BoundsSummary {
        min,
        max,
        size: [size.x, size.y, size.z],
        center: [center.x, center.y, center.z],
        diagonal: aabb.diagonal(),
    })
}
