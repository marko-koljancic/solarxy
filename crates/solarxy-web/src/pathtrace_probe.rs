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
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("solarxy pathtrace probe device"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: probe_limits(),
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
        let scene = TraceScene::upload(&device, &queue, &layouts, &arena);

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

/// A deterministic mesh large enough to make a hierarchy build measurable.
///
/// Generated rather than fetched so the harness needs no model file and every
/// run measures the same geometry. Returns `{ positions, indices, triangles }`
/// with positions as flat `xyz`, which is what
/// [`solarxy_web::build_bvh_job`](crate::build_bvh_job) takes.
#[wasm_bindgen]
pub fn bvh_corpus_mesh(width: u32, height: u32) -> Result<JsValue, JsError> {
    let (positions, indices) = corpus::sphere(width, height);
    let flat: &[f32] = bytemuck::cast_slice(&positions);
    let out = js_sys::Object::new();
    let set = |key: &str, value: &JsValue| {
        js_sys::Reflect::set(&out, &JsValue::from_str(key), value)
            .map_err(|_| JsError::new("bvh_corpus_mesh: reflect set failed"))
            .map(|_| ())
    };
    set("positions", &js_sys::Float32Array::from(flat).into())?;
    set(
        "indices",
        &js_sys::Uint32Array::from(indices.as_slice()).into(),
    )?;
    set("triangles", &JsValue::from_f64((indices.len() / 3) as f64))?;
    Ok(out.into())
}

/// Reads a packed hierarchy blob back and reports what is in it.
///
/// The harness needs this because a build that silently produced nothing takes
/// the same shape as one that worked: a blob comes back and a promise settles.
/// Returns `nodes,primitives,triangles`.
#[wasm_bindgen]
pub fn bvh_blob_summary(blob: &[u8]) -> Result<String, JsError> {
    let bvh =
        solarxy_bvh::transfer::unpack(blob).map_err(|e| JsError::new(&format!("unpack: {e}")))?;
    let stats = bvh.stats();
    Ok(format!(
        "{},{},{}",
        bvh.nodes().len(),
        bvh.prim_indices().len(),
        stats.prim_count
    ))
}

/// The limits a probe device asks for, which are **not** the shipped app's.
///
/// Core WebGPU grants eight compute-stage storage buffers and the traced scene
/// group spends seven of them. A probe binds that group and adds two of its own,
/// for the corpus it reads and the answers it writes, which is nine. Chrome
/// rejects the pipeline layout at that point and says so precisely; native wgpu
/// on Metal accepts it, and the wasm backend swallows the rejection, so the
/// dispatch runs against an invalid pipeline and every answer comes back zero.
/// That is the third time in this milestone that the browser has been the
/// authority on a limit and the desktop has not, and the first two are in the
/// gate amendment.
///
/// So a probe asks for ten, which every adapter this has met reports. **The
/// shipped kernel is unaffected and its own claim is unweakened**: it binds
/// seven and no io group, and [`BsdfProbeCheck`] builds it on a separate device
/// at exactly [`wgpu::Limits::default()`] for that reason. What a raised limit
/// buys is a harness that can hold both a scene and a question about it; what it
/// must never do is let the thing that ships stop being measured at the limits
/// it ships under.
fn probe_limits() -> wgpu::Limits {
    wgpu::Limits {
        max_storage_buffers_per_shader_stage: 10,
        ..wgpu::Limits::default()
    }
}

/// The BSDF probe and the furnace kernel, as a browser can drive them.
///
/// Its first job is pipeline creation, and that is the half only a browser can
/// answer. The lobes are the branchiest thing in the directory and the composition
/// beneath them is six fragments deep, so if either WGSL front end is going to
/// reject the uniformity discipline this is where it happens, with a validation
/// message rather than silently.
///
/// Its second job is a sanity number. It draws a small batch for a few materials
/// and reports the directional albedo, so a browser that compiles the kernel but
/// computes something else does not read as a pass.
#[wasm_bindgen]
pub struct BsdfProbeCheck {
    device: wgpu::Device,
    readback: Option<solarxy_renderer::pathtrace::probe::ColorReadback>,
    /// One entry per material, in the order the batch was built.
    roughness: Vec<f32>,
    /// Samples drawn per material.
    samples: usize,
    /// Whether the path pipelines built and dispatched.
    furnace_ok: bool,
    /// Whether the shipped path kernel builds at **core WebGPU limits**, which
    /// is a different and stricter question: the probes run on a device that
    /// asks for a raised storage-buffer limit, and the thing that ships must
    /// not.
    core_limits: Result<(), String>,
}

#[wasm_bindgen]
impl BsdfProbeCheck {
    /// Requests a device, builds both probe pipelines and the furnace pipeline,
    /// and submits a batch.
    #[wasm_bindgen]
    pub async fn create() -> Result<BsdfProbeCheck, JsError> {
        use solarxy_renderer::pathtrace::TraceAtlas;
        use solarxy_renderer::pathtrace::probe::{BsdfProbe, BsdfProbeMode, BsdfTap};

        /// Samples per material. Small on purpose: this is a compile-and-agree
        /// check in a second browser, not the sweep, which runs natively where it
        /// can fail a build.
        const SAMPLES: u32 = 1024;

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|e| JsError::new(&format!("requestAdapter: {e}")))?;
        // Exactly what both shells ask for, for the same reason as above.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("solarxy bsdf probe device"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: probe_limits(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| JsError::new(&format!("requestDevice: {e}")))?;

        let layouts = PathtraceLayouts::new(&device);

        // A white surface at four roughnesses, which is the diagonal of the furnace
        // grid and enough to tell a compiled-but-wrong kernel from a correct one.
        let roughness = vec![0.05f32, 0.35, 0.65, 0.95];
        let materials: Vec<solarxy_renderer::pathtrace::material::TracedMaterial> = roughness
            .iter()
            .map(|r| {
                let raw = solarxy_core::geometry::RawMaterialData {
                    base_color_factor: [1.0, 1.0, 1.0, 1.0],
                    roughness_factor: *r,
                    metallic_factor: 1.0,
                    ..Default::default()
                };
                solarxy_renderer::pathtrace::material::TracedMaterial::from_raw(
                    &raw,
                    &solarxy_renderer::pathtrace::scene::MaterialTextures::default(),
                )
            })
            .collect();

        let (positions, indices) = corpus::sphere(8, 4);
        let blas = Bvh::build_triangles(&positions, &indices);
        let (world, inv_world) = mat(1.0, 1.0, 1.0, [0.0, 0.0, 0.0]);
        let boxes = [transformed_bounds(&positions, &world)];
        let tlas = Bvh::build_tlas(&boxes);
        let mesh = ArenaMesh {
            bvh: &blas,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let arena = TraceArena::build(
            &tlas,
            &[mesh],
            &[ArenaPlacement {
                mesh: 0,
                world,
                inv_world,
                material_base: 0,
                flags: INSTANCE_VISIBLE,
            }],
        )
        .with_materials(materials);
        let scene = TraceScene::upload(&device, &queue, &layouts, &arena);
        let atlas = TraceAtlas::new(&device, &queue, &layouts);

        // Pipeline creation for both probe modes. This is the check.
        let probe = BsdfProbe::new(&device, &layouts);

        // And the light probe's two, which compose the environment and the
        // light fragments over the same stack. Built rather than dispatched:
        // what a browser answers here is whether it accepts the composition,
        // and the numbers those probes produce are asserted on the desktop.
        let _lights = solarxy_renderer::pathtrace::probe::LightProbe::new(&device, &layouts);

        // And the path kernel, which is the deepest composition in the
        // directory, on this device and then again on one at core limits.
        let furnace_ok = dispatch_furnace_once(&device, &queue, &layouts, &scene, &atlas);
        let core_limits = build_path_kernel_at_core_limits().await;

        let taps: Vec<BsdfTap> = (0..roughness.len() as u32)
            .flat_map(|material| {
                (0..SAMPLES).map(move |i| BsdfTap {
                    wo: [0.0, 0.0, 1.0, 0.0],
                    wi: [0.0; 4],
                    material,
                    sample_index: i,
                    strata: SAMPLES,
                    seed: 0x9E37_79B9,
                })
            })
            .collect();
        let readback = probe.submit(
            &device,
            &queue,
            BsdfProbeMode::Sample,
            &scene,
            &atlas,
            &taps,
        );

        Ok(BsdfProbeCheck {
            device,
            readback: Some(readback),
            roughness,
            samples: SAMPLES as usize,
            furnace_ok,
            core_limits,
        })
    }

    /// Polls the readback. Returns `null` while pending, else a JSON verdict.
    #[wasm_bindgen(js_name = poll)]
    pub fn poll(&mut self) -> Option<String> {
        use solarxy_renderer::pathtrace::probe::{BSDF_RESULT_WIDTH, ColorPoll};

        let readback = self.readback.as_mut()?;
        let values = match readback.poll(&self.device) {
            ColorPoll::Pending => return None,
            ColorPoll::Failed => {
                self.readback = None;
                return Some(
                    r#"{"ok":false,"error":"the bsdf readback could not be mapped"}"#.to_string(),
                );
            }
            ColorPoll::Ready(v) => v,
        };
        self.readback = None;

        let mut albedos = Vec::with_capacity(self.roughness.len());
        let mut ok = self.furnace_ok && self.core_limits.is_ok();
        for m in 0..self.roughness.len() {
            let mut sum = 0.0f64;
            for i in 0..self.samples {
                let base = (m * self.samples + i) * BSDF_RESULT_WIDTH;
                let pdf = values[base][3];
                if pdf > 0.0 {
                    sum += f64::from(values[base + 1][0]) / f64::from(pdf);
                }
            }
            let albedo = sum / self.samples as f64;
            // The same ceiling the native sweep asserts. A browser that compiles
            // the kernel and computes something else fails here.
            if albedo > 1.05 {
                ok = false;
            }
            albedos.push(albedo);
        }

        let core = match &self.core_limits {
            Ok(()) => "\"built\"".to_string(),
            Err(e) => format!("\"{}\"", e.replace('"', "'").replace('\n', " ")),
        };
        Some(format!(
            r#"{{"ok":{},"furnace":{},"coreLimits":{},"samples":{},"roughness":[{}],"albedo":[{}]}}"#,
            ok,
            self.furnace_ok,
            core,
            self.samples,
            self.roughness
                .iter()
                .map(|r| format!("{r:.2}"))
                .collect::<Vec<_>>()
                .join(","),
            albedos
                .iter()
                .map(|a| format!("{a:.4}"))
                .collect::<Vec<_>>()
                .join(","),
        ))
    }
}

/// Builds the path pipelines and dispatches one small tile through each.
///
/// Separate from the probe's own setup because it is a separate question: the probe
/// asks whether the lobes compile, and this asks whether the whole integrator does,
/// bindings and dispatch included. Sixteen by sixteen at one sample, because what is
/// being tested is that the browser accepts it, not what it draws.
///
/// All three estimators are built and dispatched, not just the one a render uses.
/// They are three specializations of one source through a pipeline-overridable
/// constant, and a browser rejecting a specialization is exactly the kind of
/// failure that arrives at pipeline creation in one front end and not the other.
fn dispatch_furnace_once(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layouts: &PathtraceLayouts,
    scene: &TraceScene,
    atlas: &solarxy_renderer::pathtrace::TraceAtlas,
) -> bool {
    use solarxy_renderer::pathtrace::{
        EnvParams, PathEstimator, PathKernel, PathUniforms, TraceParams, TraceTarget,
    };

    const EDGE: u32 = 16;

    let camera = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bsdf probe camera"),
        size: std::mem::size_of::<solarxy_renderer::camera::CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uniforms = PathUniforms::new(device, &camera);
    let kernel = PathKernel::new(device, layouts, &uniforms);
    let target = TraceTarget::new(device, layouts, EDGE, EDGE);
    uniforms.write(
        queue,
        &TraceParams {
            tile_offset: [0, 0],
            tile_size: [EDGE, EDGE],
            resolution: [EDGE, EDGE],
            bounces: 2,
            transmissive_bounces: 1,
            samples: 1,
            seed: 1,
            light_count: 0,
            // An open aperture, so the browser compiles the lens arm rather
            // than the branch that skips it.
            aperture_radius: 0.05,
            focus_distance: 4.0,
            aperture_blades: 6,
        },
        // The furnace configuration: one uniform colour, no image, no lights.
        &EnvParams::constant([0.5, 0.5, 0.5], [0.5, 0.5, 0.5]),
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("bsdf probe path encoder"),
    });
    for estimator in PathEstimator::ALL {
        kernel.encode(
            &mut encoder,
            estimator,
            scene,
            atlas,
            &target,
            &uniforms,
            [EDGE, EDGE],
        );
    }
    queue.submit(Some(encoder.finish()));
    true
}

/// Builds the shipped path kernel on a device at **core WebGPU limits**, which
/// is the claim the whole milestone rests on.
///
/// Separate from every other check here and deliberately so. The probes above
/// run on a device that asks for a raised storage-buffer limit, because a
/// harness binds the scene *and* a question about it; the thing that ships binds
/// only the scene, and if it ever stopped fitting inside what a browser grants
/// by default, everything else on this page would still pass. So this asks the
/// one question in the one configuration that matters, and reports what it
/// finds rather than that it was asked.
async fn build_path_kernel_at_core_limits() -> Result<(), String> {
    use solarxy_renderer::pathtrace::{PathKernel, PathUniforms};

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|e| format!("requestAdapter: {e}"))?;
    let (device, _queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("solarxy core-limits device"),
            required_features: wgpu::Features::empty(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            // Exactly what both shells ask for. Asking for more here would
            // prove something the shipped app cannot run.
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| format!("requestDevice: {e}"))?;

    // An error scope around the whole build, because the wasm backend does not
    // surface a rejected pipeline layout as a Rust error: it logs and carries
    // on, which is how a dead pipeline reaches a dispatch and returns zeros.
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let layouts = PathtraceLayouts::new(&device);
    let camera = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("core-limits camera"),
        size: std::mem::size_of::<solarxy_renderer::camera::CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uniforms = PathUniforms::new(&device, &camera);
    let _kernel = PathKernel::new(&device, &layouts, &uniforms);
    match device.pop_error_scope().await {
        Some(error) => Err(error.to_string()),
        None => Ok(()),
    }
}
