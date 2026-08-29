use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::ibl::{BrdfLut, IblState};
use solarxy_renderer::pipelines::Pipelines;

/// Fails the test instead of skipping it when `SOLARXY_REQUIRE_GPU=1`.
///
/// These tests skip themselves when no adapter is present, which is right on a
/// developer machine without a GPU and wrong in CI: the whole GPU suite skipped
/// silently on every run for the life of the project, so zero pixels were ever
/// verified. CI sets this on the runners that do have an adapter, turning "no
/// GPU" from an invisible skip into a red build.
fn no_adapter(reason: &str) {
    assert!(
        std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
        "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter: {reason}. \
         This runner is supposed to have one; a silent skip here is what let the \
         GPU suite go unrun for the whole project."
    );
    eprintln!("no wgpu adapter available ({reason}) — skipping");
}

fn try_get_device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::SurfaceConfiguration)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        // Honour `WGPU_BACKEND` so a backend can be pinned (and so the
        // require-GPU gate can be exercised by pointing at an absent backend).
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        ..Default::default()
    });

    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) {
        Ok(a) => a,
        Err(e) => {
            no_adapter(&format!("request_adapter failed: {e}"));
            return None;
        }
    };

    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("solarxy-renderer headless smoke"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::default(),
        })) {
            Ok(dq) => dq,
            Err(e) => {
                no_adapter(&format!("request_device failed: {e}"));
                return None;
            }
        };

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width: 256,
        height: 256,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    Some((device, queue, config))
}

#[test]
fn renderer_components_construct_without_window() {
    let Some((device, queue, config)) = try_get_device() else {
        eprintln!("no wgpu adapter available — skipping headless smoke test");
        return;
    };

    let layouts = BindGroupLayouts::new(&device);

    let _pipelines = Pipelines::new(&device, &config, &layouts, 1);

    let ibl_fallback = IblState::fallback(&device, &queue);
    assert_eq!(
        ibl_fallback.irradiance_average.len(),
        3,
        "IBL fallback should emit a 3-channel L0 average"
    );

    let _brdf_lut = BrdfLut::generate(&device, &queue);
}

/// Renders a frame with a NON-EMPTY attribute-label set and asserts wgpu
/// raised no validation error.
///
/// The label draw is gated on `vertex_count() > 0`, so the test above --
/// which builds every pipeline but draws nothing -- could not see the bug
/// that shipped in 0.8.1: a WGSL padding field with the wrong alignment made
/// the shader's uniform struct 112 bytes against the Rust struct's 96, which
/// wgpu only checks at DRAW time. The encoder was invalidated, the pane's
/// whole frame (composite pass included) was discarded, and the viewport
/// went black with no Rust error anywhere.
///
/// `tests/uniform_layout.rs` catches that specific class cheaply and without
/// a GPU. This one is the backstop for everything else that can only fail
/// once a real draw is submitted.
#[test]
fn a_frame_with_labels_submits_without_validation_errors() {
    use solarxy_core::preferences::{
        BackgroundMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
        UvMapBackground, UvMode, ViewMode,
    };
    use solarxy_core::view_config::{BoundsMode, PaneDisplaySettings};
    use solarxy_core::AABB;
    use solarxy_renderer::camera_state::CameraState;
    use solarxy_renderer::environment::SceneEnvironment;
    use solarxy_renderer::frame::{Renderer, RendererInit};
    use solarxy_renderer::labels::{LabelInstance, LabelStyle, pack_glyph};
    use solarxy_renderer::visualization::VisualizationState;

    let Some((device, queue, config)) = try_get_device() else {
        return;
    };

    // Any validation error inside this scope fails the test with wgpu's own
    // message, rather than being printed to stderr and ignored.
    device.push_error_scope(wgpu::ErrorFilter::Validation);

    let init = RendererInit {
        // The main pass always resolves, so the HDR source has to be
        // multisampled; 4 is what the golden harness uses.
        msaa_sample_count: 4,
        gradient_top: [0.1, 0.1, 0.12, 1.0],
        gradient_bottom: [0.02, 0.02, 0.03, 1.0],
        sky_top: [0.4, 0.5, 0.7],
        sky_bottom: [0.2, 0.2, 0.25],
        wireframe_color: [1.0, 1.0, 1.0, 1.0],
        wireframe_line_width: 1.0,
        bloom_enabled: false,
        ssao_enabled: false,
        tone_mode: solarxy_core::preferences::ToneMode::AcesFilmic,
        exposure: 1.0,
        ibl_mode: solarxy_core::preferences::IblMode::Full,
        uv_checker_png: include_bytes!("../../../res/textures/uv-checker_1k.png"),
    };
    let mut renderer = Renderer::new(&device, &queue, &config, &init).expect("headless renderer");

    let bounds = AABB {
        min: cgmath::Point3::new(-1.0, -1.0, -1.0),
        max: cgmath::Point3::new(1.0, 1.0, 1.0),
    };
    let vis = VisualizationState::new_from_parts(
        &device,
        &renderer.layouts,
        &bounds,
        &[],
        None,
        [0.3, 0.3, 0.3],
    );
    let env = SceneEnvironment::new(
        &device,
        &queue,
        &renderer.layouts,
        &bounds,
        1.0,
        &renderer.ibl_res.brdf_lut,
        &renderer.ibl_res.ltc,
        1024,
        vis,
    );
    let mut cam = CameraState::new(&device, &renderer.layouts.camera, &bounds, 1.0);
    cam.update(&queue, 1.0 / 60.0);

    // Two labels, each one glyph: enough that `vertex_count()` is non-zero
    // and the draw actually happens.
    let instances = [
        LabelInstance {
            pos: [0.0, 0.0, 0.0],
            glyph_count: 1,
        },
        LabelInstance {
            pos: [0.5, 0.5, 0.0],
            glyph_count: 1,
        },
    ];
    let glyphs = [pack_glyph(0, 0, 0), pack_glyph(1, 0, 1)];

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("label smoke target"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let _target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let pds = PaneDisplaySettings {
        view_mode: ViewMode::Shaded,
        prev_non_ghosted_mode: ViewMode::Shaded,
        ghosted_wireframe: false,
        normals_mode: NormalsMode::Off,
        background_mode: BackgroundMode::GRADIENT,
        uv_mode: UvMode::Off,
        bounds_mode: BoundsMode::Off,
        line_weight: LineWeight::Medium,
        show_grid: true,
        show_axis_gizmo: false,
        show_local_axes: false,
        inspection_mode: InspectionMode::Shaded,
        material_override: MaterialOverride::None,
        texel_density_target: 1.0,
        pane_mode: PaneMode::Scene3D,
        uv_bg: UvMapBackground::Dark,
        uv_offset: [0.0, 0.0],
        uv_zoom: 1.0,
        show_uv_overlap: false,
        show_validation: false,
        turntable_active: false,
        pane_engine: solarxy_core::view_config::PaneEngine::Raster,
    };

    // Both background modes of the label channel, because the chip flag
    // shifts the vertex ranges the shader decodes and the two are separate
    // code paths through `vs_label`.
    for background in [
        solarxy_renderer::labels::LabelBackground::Chip,
        solarxy_renderer::labels::LabelBackground::None,
    ] {
        renderer.write_label_style(
            &queue,
            &LabelStyle {
                background,
                ..LabelStyle::new_default()
            },
        );
        renderer.set_attr_labels(&device, &queue, &instances, &glyphs);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("label smoke encoder"),
        });
        renderer.render_main_pass(
            &mut encoder,
            &env,
            &[],
            &cam.bind_group,
            &cam.camera,
            &pds,
            BackgroundMode::GRADIENT.resolve(&[]),
        );
        queue.submit(Some(encoder.finish()));
    }

    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let error = pollster::block_on(device.pop_error_scope());
    assert!(
        error.is_none(),
        "drawing attribute labels raised a wgpu validation error: {error:?}\n\
         A uniform-size mismatch between a WGSL struct and its Rust mirror \
         shows up exactly here, and in the app it renders as a black \
         viewport rather than an error. See tests/uniform_layout.rs."
    );
}

/// Drives the host-facing preference setters inside a validation error
/// scope, so a buffer created without the usage its setter's write needs
/// fails a Rust test instead of a user's session.
///
/// The class this pins: `queue.write_buffer` against a buffer created
/// without `COPY_DST` is rejected at validation time, the buffer keeps its
/// creation contents, and the only symptom is a console line -- or, on the
/// desktop, a panic from wgpu's default uncaptured-error handler. The
/// selection tint's colour buffer shipped exactly that way: every browser
/// boot logged the error and the tint stayed at its hard-coded creation
/// colour whatever the preference said. Setters whose only caller is one
/// shell are the exposed ones, which is why this exercises the setters
/// generally rather than naming that buffer.
#[test]
fn preference_setters_write_without_validation_errors() {
    use solarxy_renderer::frame::{Renderer, RendererInit, SelectionStyle};
    use solarxy_renderer::labels::LabelStyle;

    let Some((device, queue, config)) = try_get_device() else {
        return;
    };

    device.push_error_scope(wgpu::ErrorFilter::Validation);

    let init = RendererInit {
        msaa_sample_count: 4,
        gradient_top: [0.1, 0.1, 0.12, 1.0],
        gradient_bottom: [0.02, 0.02, 0.03, 1.0],
        sky_top: [0.4, 0.5, 0.7],
        sky_bottom: [0.2, 0.2, 0.25],
        wireframe_color: [1.0, 1.0, 1.0, 1.0],
        wireframe_line_width: 1.0,
        bloom_enabled: false,
        ssao_enabled: false,
        tone_mode: solarxy_core::preferences::ToneMode::AcesFilmic,
        exposure: 1.0,
        ibl_mode: solarxy_core::preferences::IblMode::Full,
        uv_checker_png: include_bytes!("../../../res/textures/uv-checker_1k.png"),
    };
    let mut renderer = Renderer::new(&device, &queue, &config, &init).expect("headless renderer");

    // Every style, because the buffer writes happen regardless of which
    // style is active.
    for style in [
        SelectionStyle::Outline,
        SelectionStyle::Tint,
        SelectionStyle::None,
    ] {
        renderer.set_selection_highlight(&queue, style, [0.9, 0.4, 0.1, 1.0], 3.0);
    }
    renderer.write_label_style(&queue, &LabelStyle::new_default());
    renderer.write_label_dpr(&queue, 2.0);

    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let error = pollster::block_on(device.pop_error_scope());
    assert!(
        error.is_none(),
        "a host-facing setter raised a wgpu validation error: {error:?}\n\
         The usual cause is a buffer created without COPY_DST and then \
         written with queue.write_buffer: the write is rejected and the \
         buffer silently keeps its creation contents."
    );
}

/// The colour-grading slots, rendered rather than reasoned about.
///
/// Three things are asserted, and the third is the one worth having:
///
/// 1. An identity table in the display-referred slot is a no-op. That is
///    the slot's whole contract.
/// 2. A slot at zero strength is a no-op whatever it holds, because a
///    grade nobody asked for must not cost a pixel.
/// 3. **The two slots are not symmetric.** An identity table in the
///    pre-tone-map slot is NOT a no-op, and that is by design rather than
///    by accident: that slot is fed log-encoded light and its output is
///    taken as the tone curve's result, so the identity table returns the
///    log encoding itself. The table that would be a no-op there is the
///    shaper's inverse, not a coordinate ramp. Pinned here because it
///    reads like a bug to anyone who meets it without this note.
#[test]
fn the_two_grading_slots_behave_as_specified() {
    use solarxy_core::LutCube;
    use solarxy_core::preferences::{
        BackgroundMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
        UvMapBackground, UvMode, ViewMode,
    };
    use solarxy_core::view_config::{BoundsMode, PaneDisplaySettings};
    use solarxy_core::AABB;
    use solarxy_renderer::camera_state::CameraState;
    use solarxy_renderer::composite::CompositeLook;
    use solarxy_renderer::environment::SceneEnvironment;
    use solarxy_renderer::frame::{Renderer, RendererInit};
    use solarxy_renderer::lut::LutSlot;
    use solarxy_renderer::visualization::VisualizationState;

    const W: u32 = 64;
    const H: u32 = 64;

    let Some((device, queue, mut config)) = try_get_device() else {
        return;
    };
    config.width = W;
    config.height = H;

    device.push_error_scope(wgpu::ErrorFilter::Validation);

    let init = RendererInit {
        msaa_sample_count: 4,
        // A gradient background gives the composite pass a wide, smooth
        // range of values to transform, which is what makes an identity
        // table's no-op claim worth anything: a flat colour would pass
        // through almost any table unchanged.
        gradient_top: [0.9, 0.5, 0.2, 1.0],
        gradient_bottom: [0.02, 0.05, 0.3, 1.0],
        sky_top: [0.4, 0.5, 0.7],
        sky_bottom: [0.2, 0.2, 0.25],
        wireframe_color: [1.0, 1.0, 1.0, 1.0],
        wireframe_line_width: 1.0,
        bloom_enabled: false,
        ssao_enabled: false,
        tone_mode: solarxy_core::preferences::ToneMode::AcesFilmic,
        exposure: 1.0,
        ibl_mode: solarxy_core::preferences::IblMode::Full,
        uv_checker_png: include_bytes!("../../../res/textures/uv-checker_1k.png"),
    };
    let mut renderer = Renderer::new(&device, &queue, &config, &init).expect("headless renderer");

    let bounds = AABB {
        min: cgmath::Point3::new(-1.0, -1.0, -1.0),
        max: cgmath::Point3::new(1.0, 1.0, 1.0),
    };
    let vis = VisualizationState::new_from_parts(
        &device,
        &renderer.layouts,
        &bounds,
        &[],
        None,
        [0.3, 0.3, 0.3],
    );
    let env = SceneEnvironment::new(
        &device,
        &queue,
        &renderer.layouts,
        &bounds,
        1.0,
        &renderer.ibl_res.brdf_lut,
        &renderer.ibl_res.ltc,
        1024,
        vis,
    );
    let mut cam = CameraState::new(&device, &renderer.layouts.camera, &bounds, 1.0);
    cam.update(&queue, 1.0 / 60.0);

    let pds = PaneDisplaySettings {
        view_mode: ViewMode::Shaded,
        prev_non_ghosted_mode: ViewMode::Shaded,
        ghosted_wireframe: false,
        normals_mode: NormalsMode::Off,
        background_mode: BackgroundMode::GRADIENT,
        uv_mode: UvMode::Off,
        bounds_mode: BoundsMode::Off,
        line_weight: LineWeight::Medium,
        show_grid: false,
        show_axis_gizmo: false,
        show_local_axes: false,
        inspection_mode: InspectionMode::Shaded,
        material_override: MaterialOverride::None,
        texel_density_target: 1.0,
        pane_mode: PaneMode::Scene3D,
        uv_bg: UvMapBackground::Dark,
        uv_offset: [0.0, 0.0],
        uv_zoom: 1.0,
        show_uv_overlap: false,
        show_validation: false,
        turntable_active: false,
        pane_engine: solarxy_core::view_config::PaneEngine::Raster,
    };

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grading target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let padded = solarxy_renderer::capture::padded_row_bytes(W);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grading readback"),
        size: u64::from(padded) * u64::from(H),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // Render the scene once into the HDR target, then composite it as many
    // times as there are looks to try. The main pass is deliberately
    // outside the loop: it is the constant, and every difference below has
    // to come from the composite pass.
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("grading main"),
    });
    renderer.render_main_pass(
        &mut encoder,
        &env,
        &[],
        &cam.bind_group,
        &cam.camera,
        &pds,
        BackgroundMode::GRADIENT.resolve(&[]),
    );
    queue.submit(Some(encoder.finish()));

    let composite_to_bytes = |renderer: &Renderer, look: &CompositeLook| -> Vec<u8> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grading composite"),
        });
        renderer.post.composite.write_params(
            &queue,
            false,
            false,
            look,
            &renderer.post.luts,
            InspectionMode::Shaded,
        );
        renderer.post.composite.render(
            &mut encoder,
            &renderer.pipelines,
            &target_view,
            false,
            &renderer.post.ssao,
            Some([0.0, 0.0, W as f32, H as f32]),
            true,
            None,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(H),
                },
            },
            wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        let bytes = slice.get_mapped_range().to_vec();
        readback.unmap();
        bytes
    };

    /// The number of bytes differing by more than `tolerance`.
    fn differing(a: &[u8], b: &[u8], tolerance: u8) -> usize {
        a.iter()
            .zip(b)
            .filter(|(x, y)| x.abs_diff(**y) > tolerance)
            .count()
    }

    // Strengths are explicit: a resolved look defaults to contributing no
    // table at all (see `CompositeLook`), so a test that wants one has to
    // ask, exactly as the camera path does.
    let neutral = CompositeLook::default();
    let full = CompositeLook {
        lut_a_strength: 1.0,
        lut_b_strength: 1.0,
        ..neutral
    };
    let plain = composite_to_bytes(&renderer, &neutral);
    assert!(
        plain.iter().any(|&b| b != 0),
        "the composite pass produced an entirely black frame, so nothing below is a test of anything"
    );

    let identity = LutCube::identity(33);

    // 1. The display-referred slot with an identity table is a no-op.
    //    Tolerance one, not zero: the table is stored at half precision
    //    and interpolated, so a sample can land a unit of last place off
    //    before the 8-bit swapchain quantizes it.
    renderer.set_lut(&device, &queue, LutSlot::B, Some(&identity));
    let through_b = composite_to_bytes(&renderer, &full);
    let diff_b = differing(&plain, &through_b, 1);
    assert_eq!(
        diff_b,
        0,
        "an identity table in the display-referred slot changed {diff_b} of {} bytes",
        plain.len()
    );

    // 2. Zero strength is a no-op no matter what the slot holds.
    let silent = CompositeLook {
        lut_b_strength: 0.0,
        ..full
    };
    let through_silent = composite_to_bytes(&renderer, &silent);
    assert_eq!(
        differing(&plain, &through_silent, 0),
        0,
        "a slot at zero strength still changed the image"
    );
    renderer.set_lut(&device, &queue, LutSlot::B, None);

    // 3. The pre-tone-map slot is NOT symmetric with it. See this test's
    //    doc comment: an identity table there returns the log encoding,
    //    which is a large, obvious change rather than a subtle one.
    renderer.set_lut(&device, &queue, LutSlot::A, Some(&identity));
    let through_a = composite_to_bytes(&renderer, &full);
    let diff_a = differing(&plain, &through_a, 1);
    assert!(
        diff_a > plain.len() / 10,
        "an identity table in the pre-tone-map slot changed only {diff_a} of {} bytes; \
         it is fed log-encoded light and should return the log encoding, so a near-no-op \
         here means the shaper is not being applied",
        plain.len()
    );
    renderer.set_lut(&device, &queue, LutSlot::A, None);

    // 4. A table bound for one pane must not leak into a path that did not
    //    ask for one. The slots are renderer-global and the look is per
    //    pane, so this is what keeps a graded viewport out of the asset
    //    preview and out of every golden capture.
    renderer.set_lut(&device, &queue, LutSlot::B, Some(&identity));
    let unasked = composite_to_bytes(&renderer, &neutral);
    assert_eq!(
        differing(&plain, &unasked, 0),
        0,
        "a bound table applied to a look that requested none"
    );
    renderer.set_lut(&device, &queue, LutSlot::B, None);

    // 5. Clearing both slots returns exactly the original frame, which is
    //    what makes the grade safe to leave wired up.
    let cleared = composite_to_bytes(&renderer, &neutral);
    assert_eq!(
        differing(&plain, &cleared, 0),
        0,
        "clearing the grading slots did not restore the original frame"
    );

    let error = pollster::block_on(device.pop_error_scope());
    assert!(
        error.is_none(),
        "the grading composite raised a wgpu validation error: {error:?}"
    );
}
