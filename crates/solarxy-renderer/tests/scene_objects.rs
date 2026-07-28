//! Headless integration tests for `SceneObjects::apply`: upsert, the
//! in-place fast path, capacity growth, transform/visibility/remove ops,
//! and light-list handling. Skips (with a note) when no GPU adapter is
//! available, so a developer machine without one stays green -- but see
//! `require_gpu!`: under `SOLARXY_REQUIRE_GPU=1` (which CI sets on the runners
//! that have an adapter) a missing GPU is a hard failure, not a skip.

use std::sync::Arc;

use solarxy_core::scene::{CookedGeometry, LightDef, LightKind, SceneDelta, SceneObjectId, SceneOp};
use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::scene_objects::{SceneObjects, cooked_from_parts};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layouts: BindGroupLayouts,
}

fn gpu() -> Option<Gpu> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        // Honour `WGPU_BACKEND` so a backend can be pinned (and so the
        // require-GPU gate can be exercised by pointing at an absent backend).
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::empty(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    let layouts = BindGroupLayouts::new(&device);
    Some(Gpu {
        device,
        queue,
        layouts,
    })
}

/// Yields a GPU, or skips the test -- unless `SOLARXY_REQUIRE_GPU=1`, in which
/// case a missing adapter is a hard failure.
///
/// The skip is right on a developer machine without a GPU and wrong in CI: this
/// whole suite skipped silently on every CI run for the life of the project (the
/// only test leg was a GPU-less ubuntu runner), so no pixel was ever verified.
/// CI sets the env var on runners that do have an adapter.
macro_rules! require_gpu {
    () => {
        match gpu() {
            Some(g) => g,
            None => {
                assert!(
                    std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                    "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter. This runner is \
                     supposed to have one; a silent skip is what let this suite go \
                     unrun for the whole project."
                );
                eprintln!("skipping: no GPU adapter available");
                return;
            }
        }
    };
}

/// A triangle with `n` vertices' worth of padding to steer buffer sizes.
fn tri(scale: f32) -> CookedGeometry {
    cooked_from_parts(
        "tri",
        vec![[0.0, 0.0, 0.0], [scale, 0.0, 0.0], [0.0, scale, 0.0]],
        vec![0, 1, 2],
        None,
    )
}

/// A denser mesh (a quad fan) that exceeds the triangle's buffer capacity.
fn quad_fan() -> CookedGeometry {
    cooked_from_parts(
        "fan",
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.5, 1.0],
            [1.5, 0.5, 1.0],
        ],
        vec![0, 1, 2, 0, 2, 3, 0, 3, 4, 3, 5, 4],
        None,
    )
}

fn upsert(id: u64, geometry: CookedGeometry) -> SceneDelta {
    SceneDelta {
        ops: vec![SceneOp::UpsertGeometry {
            id: SceneObjectId(id),
            geometry: Arc::new(geometry),
        }],
    }
}

#[test]
fn upsert_creates_object_with_expected_counts() {
    let g = require_gpu!();
    let mut scene = SceneObjects::new();
    scene
        .apply(&g.device, &g.queue, &g.layouts, &upsert(1, tri(1.0)))
        .expect("apply");

    assert_eq!(scene.len(), 1);
    let obj = scene.get(SceneObjectId(1)).expect("object exists");
    assert_eq!(obj.model.meshes.len(), 1);
    assert_eq!(obj.model.meshes[0].num_elements, 3);
    // Clay default material synthesized for material-less geometry.
    assert_eq!(obj.model.materials.len(), 1);
    assert!(obj.visible);
    assert_eq!(obj.model.cpu_meshes[0].positions.len(), 3);
}

#[test]
fn in_place_rewrite_updates_counts_and_bounds() {
    let g = require_gpu!();
    let mut scene = SceneObjects::new();
    scene
        .apply(&g.device, &g.queue, &g.layouts, &upsert(1, tri(1.0)))
        .expect("first upsert");

    // Same topology, scaled positions: fits capacity, rewrites in place.
    scene
        .apply(&g.device, &g.queue, &g.layouts, &upsert(1, tri(2.0)))
        .expect("in-place upsert");

    let obj = scene.get(SceneObjectId(1)).expect("object");
    assert_eq!(scene.len(), 1);
    assert_eq!(obj.model.meshes[0].num_elements, 3);
    assert!((obj.model.bounds.max.x - 2.0).abs() < 1e-6, "bounds follow");
    assert!((obj.model.cpu_meshes[0].positions[1][0] - 2.0).abs() < 1e-6);
}

#[test]
fn capacity_overflow_rebuilds_and_preserves_transform() {
    let g = require_gpu!();
    let mut scene = SceneObjects::new();
    scene
        .apply(&g.device, &g.queue, &g.layouts, &upsert(1, tri(1.0)))
        .expect("first upsert");

    // Move it, then grow beyond the triangle's 1.5x headroom.
    let translate = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [3.0, 4.0, 5.0, 1.0],
    ];
    scene
        .apply(
            &g.device,
            &g.queue,
            &g.layouts,
            &SceneDelta {
                ops: vec![SceneOp::SetTransform {
                    id: SceneObjectId(1),
                    transform: translate,
                }],
            },
        )
        .expect("transform");
    scene
        .apply(&g.device, &g.queue, &g.layouts, &upsert(1, quad_fan()))
        .expect("growth upsert");

    let obj = scene.get(SceneObjectId(1)).expect("object");
    assert_eq!(obj.model.meshes[0].num_elements, 12);
    // The rebuild kept the transform.
    assert!((obj.transform.w.x - 3.0).abs() < 1e-6);
    assert!((obj.transform.w.y - 4.0).abs() < 1e-6);
}

#[test]
fn visibility_remove_and_clear() {
    let g = require_gpu!();
    let mut scene = SceneObjects::new();
    scene
        .apply(&g.device, &g.queue, &g.layouts, &upsert(1, tri(1.0)))
        .expect("obj 1");
    scene
        .apply(&g.device, &g.queue, &g.layouts, &upsert(2, tri(1.0)))
        .expect("obj 2");

    scene
        .apply(
            &g.device,
            &g.queue,
            &g.layouts,
            &SceneDelta {
                ops: vec![SceneOp::SetVisible {
                    id: SceneObjectId(1),
                    visible: false,
                }],
            },
        )
        .expect("hide");
    assert!(!scene.get(SceneObjectId(1)).expect("obj").visible);
    // visible_bounds skips hidden objects but keeps visible ones.
    assert!(scene.visible_bounds().is_some());

    scene
        .apply(
            &g.device,
            &g.queue,
            &g.layouts,
            &SceneDelta {
                ops: vec![SceneOp::Remove {
                    id: SceneObjectId(2),
                }],
            },
        )
        .expect("remove");
    assert_eq!(scene.len(), 1);
    assert!(scene.visible_bounds().is_none(), "only hidden object left");

    scene
        .apply(
            &g.device,
            &g.queue,
            &g.layouts,
            &SceneDelta {
                ops: vec![SceneOp::Clear],
            },
        )
        .expect("clear");
    assert!(scene.is_empty());
}

#[test]
fn set_lights_stores_and_flags_dirty_once() {
    let g = require_gpu!();
    let mut scene = SceneObjects::new();
    assert!(scene.lights().is_none());
    assert!(!scene.take_lights_dirty());

    let light = LightDef {
        kind: LightKind::Point,
        position: [0.0, 5.0, 0.0],
        direction: [0.0, -1.0, 0.0],
        color: [1.0, 1.0, 1.0],
        intensity: 2.0,
        range: 0.0,
        decay: 0.0,
        inner_cone: 0.0,
        outer_cone: 0.0,
        area_extent: [0.0, 0.0],
        rotate: [0.0; 3],
        two_sided: false,
        ground_color: [0.0, 0.0, 0.0],
        cast_shadow: true,
        shadow_map_size: 1024,
        shadow_bias: 0.0,
        visible: true,
        show_helper: false,
        helper_size: 1.0,
    };
    scene
        .apply(
            &g.device,
            &g.queue,
            &g.layouts,
            &SceneDelta {
                ops: vec![SceneOp::SetLights {
                    lights: vec![light],
                }],
            },
        )
        .expect("lights");

    assert_eq!(scene.lights().map(<[LightDef]>::len), Some(1));
    assert!(scene.take_lights_dirty());
    assert!(!scene.take_lights_dirty(), "dirty consumed once");
}

#[test]
fn iteration_order_is_deterministic_by_id() {
    let g = require_gpu!();
    let mut scene = SceneObjects::new();
    for id in [5u64, 1, 3] {
        scene
            .apply(&g.device, &g.queue, &g.layouts, &upsert(id, tri(1.0)))
            .expect("upsert");
    }
    let order: Vec<u64> = scene.iter().map(|(id, _)| id.0).collect();
    assert_eq!(order, vec![1, 3, 5]);
}
