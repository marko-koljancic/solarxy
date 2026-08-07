//! The browser side of the traversal parity check.
//!
//! The desktop and the browser run different WGSL front ends against the same
//! specification, and this codebase has already lost time to one accepting what
//! the other rejects. So the check the native test runs also runs here: build
//! the pipeline, drive a corpus of rays through the WGSL traversal, drive the
//! same corpus through the CPU traversal in the same wasm instance, and report
//! whether they agree.
//!
//! Feature-gated off, so the shipped artifact carries neither this nor the
//! corpus generator it pulls in. Build the smoke harness with it on:
//!
//! ```text
//! bash crates/solarxy-web/build-wasm.sh crates/solarxy-web/smoke/pkg --features pt-probe
//! ```
//!
//! It owns its own device rather than borrowing the app's. The check is about
//! whether a device configured the way both shells configure one accepts the
//! kernel, and a diagnostic that took the editor's device down with it would be
//! a bad trade for a diagnostic.
//!
//! The readback is polled rather than awaited because that is the contract the
//! renderer's readbacks offer: WebGPU has no blocking wait, so `poll` returns
//! pending and the caller comes back. JS drives the loop.

use solarxy_bvh::{Bvh, Instanced, corpus};
use solarxy_core::aabb::AABB;
use solarxy_renderer::bind_groups::PathtraceLayouts;
use solarxy_renderer::pathtrace::TraceScene;
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::probe::{CorpusRay, HitPoll, HitReadback, TraversalProbe};
use wasm_bindgen::prelude::*;

/// The ray budget both sides pass. Matches the native test, and is finite for
/// the same reason: WGSL has no infinity literal, and a finite bound on one
/// side against an infinite one on the other is a difference between the
/// implementations rather than between their answers.
const T_MAX: f32 = 1e30;

/// Rays in the corpus. Small: this is a compile-and-agree check in a second
/// browser, not the exhaustive sweep, which runs natively where it can fail a
/// build.
const RAY_COUNT: u32 = 2048;

/// The traversal probe, as a browser can drive it.
#[wasm_bindgen]
pub struct PathtraceProbe {
    device: wgpu::Device,
    scene: TraceScene,
    rays: Vec<corpus::CorpusRay>,
    expected: Vec<Option<solarxy_bvh::InstanceHit>>,
    expected_occluded: Vec<bool>,
    readback: Option<HitReadback>,
}

#[wasm_bindgen]
impl PathtraceProbe {
    /// Requests a device, builds the pipeline, and submits the corpus.
    ///
    /// Pipeline creation is the half of this the browser is uniquely able to
    /// answer, so a front end that rejects the traversal fails here with the
    /// validation message rather than silently.
    #[wasm_bindgen]
    pub async fn create() -> Result<PathtraceProbe, JsError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|e| JsError::new(&format!("requestAdapter: {e}")))?;
        // Exactly what both shells ask for. Asking for more here would prove
        // something the shipped app cannot run.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("solarxy pathtrace probe device"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| JsError::new(&format!("requestDevice: {e}")))?;

        let layouts = PathtraceLayouts::new(&device);
        let (positions, indices) = corpus::sphere(24, 12);
        let blas = Bvh::build_triangles(&positions, &indices);

        // Two placements, one of them non-uniformly scaled: the case that fails
        // if the transformed ray direction is renormalized.
        let identity = mat(1.0, 1.0, 1.0, [0.0, 0.0, 0.0]);
        let scaled = mat(0.6, 0.25, 0.4, [1.4, 0.0, 0.0]);
        let placements = [identity, scaled];

        let boxes: Vec<AABB> = placements
            .iter()
            .map(|(world, _)| transformed_bounds(&positions, world))
            .collect();
        let tlas = Bvh::build_tlas(&boxes);

        let mesh = ArenaMesh {
            bvh: &blas,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let arena_placements: Vec<ArenaPlacement> = placements
            .iter()
            .map(|(world, inv_world)| ArenaPlacement {
                mesh: 0,
                world: *world,
                inv_world: *inv_world,
                material_base: 0,
                flags: INSTANCE_VISIBLE,
            })
            .collect();
        let arena = TraceArena::build(&tlas, &[mesh], &arena_placements);
        let scene = TraceScene::upload(&device, &layouts, &arena);

        let rays = corpus::rays(0x2545_F491, RAY_COUNT);
        let gpu_rays: Vec<CorpusRay> = rays
            .iter()
            .map(|r| CorpusRay {
                origin: [r.origin[0], r.origin[1], r.origin[2], 0.0],
                direction: [r.direction[0], r.direction[1], r.direction[2], 0.0],
            })
            .collect();

        // The CPU answers are computed now, while the GPU work is in flight.
        let instances: Vec<Instanced<'_>> = placements
            .iter()
            .map(|(_, inv_world)| Instanced {
                inv_world: *inv_world,
                blas: &blas,
                positions: &positions,
                indices: &indices,
            })
            .collect();
        let expected: Vec<_> = rays
            .iter()
            .map(|r| tlas.intersect_instances(r.origin, r.direction, T_MAX, &instances))
            .collect();
        let expected_occluded: Vec<bool> = rays
            .iter()
            .map(|r| tlas.occluded_instances(r.origin, r.direction, T_MAX, &instances))
            .collect();

        let probe = TraversalProbe::new(&device, &layouts.scene);
        let readback = probe.submit(&device, &queue, &scene, &gpu_rays);

        Ok(PathtraceProbe {
            device,
            scene,
            rays,
            expected,
            expected_occluded,
            readback: Some(readback),
        })
    }

    /// Polls the readback. Returns `null` while pending, else a JSON verdict.
    ///
    /// JS drives this with a timer rather than an animation frame: an occluded
    /// or backgrounded tab gets no animation frames at all, and a harness that
    /// awaited one would hang instead of reporting.
    #[wasm_bindgen(js_name = poll)]
    pub fn poll(&mut self) -> Option<String> {
        let readback = self.readback.as_mut()?;
        let hits = match readback.poll(&self.device) {
            HitPoll::Pending => return None,
            HitPoll::Failed => {
                self.readback = None;
                return Some(
                    r#"{"ok":false,"error":"the corpus readback could not be mapped"}"#.to_string(),
                );
            }
            HitPoll::Ready(hits) => hits,
        };
        self.readback = None;

        let mut compared = 0u32;
        let mut hit_count = 0u32;
        let mut mismatches: Vec<String> = Vec::new();
        for ((ray, want), have) in self.rays.iter().zip(&self.expected).zip(&hits) {
            compared += 1;
            if want.is_some() != have.hit() {
                if mismatches.len() < 4 {
                    mismatches.push(format!(
                        "ray {} disagrees on whether anything was hit",
                        ray.index
                    ));
                }
                continue;
            }
            if let Some(want) = want {
                hit_count += 1;
                if (want.t - have.t).abs() >= 1e-3 && mismatches.len() < 4 {
                    mismatches.push(format!(
                        "ray {}: distance {} versus {}",
                        ray.index, want.t, have.t
                    ));
                }
            }
        }
        for ((ray, want), have) in self
            .rays
            .iter()
            .zip(&self.expected_occluded)
            .zip(&hits)
            .filter(|((_, w), h)| **w != h.occluded())
        {
            let _ = have;
            if mismatches.len() < 8 {
                mismatches.push(format!(
                    "ray {}: any-hit disagrees ({want} versus the kernel)",
                    ray.index
                ));
            }
        }

        Some(format!(
            r#"{{"ok":{},"rays":{},"hits":{},"instances":{},"mismatches":[{}]}}"#,
            mismatches.is_empty(),
            compared,
            hit_count,
            self.scene.instance_count(),
            mismatches
                .iter()
                .map(|m| format!("\"{}\"", m.replace('"', "'")))
                .collect::<Vec<_>>()
                .join(","),
        ))
    }
}

/// A scale-then-translate placement and its inverse, column-major.
fn mat(sx: f32, sy: f32, sz: f32, t: [f32; 3]) -> ([[f32; 4]; 4], [[f32; 4]; 4]) {
    let world = [
        [sx, 0.0, 0.0, 0.0],
        [0.0, sy, 0.0, 0.0],
        [0.0, 0.0, sz, 0.0],
        [t[0], t[1], t[2], 1.0],
    ];
    let inv = [
        [1.0 / sx, 0.0, 0.0, 0.0],
        [0.0, 1.0 / sy, 0.0, 0.0],
        [0.0, 0.0, 1.0 / sz, 0.0],
        [-t[0] / sx, -t[1] / sy, -t[2] / sz, 1.0],
    ];
    (world, inv)
}

fn transformed_bounds(positions: &[[f32; 3]], world: &[[f32; 4]; 4]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        let q = [
            world[0][0] * p[0] + world[1][0] * p[1] + world[2][0] * p[2] + world[3][0],
            world[0][1] * p[0] + world[1][1] * p[1] + world[2][1] * p[2] + world[3][1],
            world[0][2] * p[0] + world[1][2] * p[1] + world[2][2] * p[2] + world[3][2],
        ];
        for axis in 0..3 {
            min[axis] = min[axis].min(q[axis]);
            max[axis] = max[axis].max(q[axis]);
        }
    }
    AABB {
        min: cgmath::Point3::new(min[0], min[1], min[2]),
        max: cgmath::Point3::new(max[0], max[1], max[2]),
    }
}
