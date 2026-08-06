//! The bundled sample scenes (web/public/samples/*.slxy) must load and
//! cook clean on the current engine. They are committed binary fixtures a
//! learner opens from File > Sample Scenes, so a node rename or cook
//! regression that breaks one must fail CI here rather than surface as a
//! broken lesson in the app.
//!
//! Skips (loudly) when the samples directory is absent, so a crates-only
//! checkout still passes.

use solarxy_graph::engine::{Engine, EngineEvent};

fn samples_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/public/samples")
}

#[test]
fn every_bundled_sample_loads_and_cooks_clean() {
    let dir = samples_dir();
    if !dir.is_dir() {
        eprintln!("skipping: no samples directory at {}", dir.display());
        return;
    }
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("samples dir readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("slxy"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "the samples directory exists but holds no .slxy files"
    );

    for path in files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let bytes = std::fs::read(&path).expect("sample readable");
        let mut engine = Engine::new().expect("builtin registry");
        let loaded = engine
            .load_slxy(&bytes)
            .unwrap_or_else(|e| panic!("{name}: failed to load: {e}"));
        assert!(
            loaded.warnings.is_empty(),
            "{name}: loaded with warnings: {:?}",
            loaded.warnings
        );

        // Drive the cook to quiescence (the samples are fully parametric,
        // no async import jobs), collecting every per-node status.
        let mut errors: Vec<String> = Vec::new();
        for _ in 0..8 {
            let events = engine.cook(&mut || true);
            if events.is_empty() {
                break;
            }
            for ev in events {
                if let EngineEvent::CookStatus { node, status } = ev
                    && let solarxy_graph::cook::state::CookStatus::Error { message } = status
                {
                    errors.push(format!("{name}: node {node:?}: {message}"));
                }
            }
        }
        assert!(errors.is_empty(), "cook errors:\n{}", errors.join("\n"));
        assert!(
            !engine.display_geometries().is_empty(),
            "{name}: no displayed geometry after cooking"
        );
    }
}

/// The animated sample must actually animate.
///
/// The sweep above proves every scene loads and cooks, which an entirely
/// static file would also pass. This is the property that makes
/// `animated-field.slxy` worth shipping: seeking the clock has to move the
/// geometry. A wrangle whose `$T` reference was lost in a refactor would
/// still cook clean, still render, and teach nothing.
#[test]
fn the_animated_sample_moves_when_the_clock_does() {
    let path = samples_dir().join("animated-field.slxy");
    if !path.is_file() {
        eprintln!("skipping: no animated-field.slxy at {}", path.display());
        return;
    }
    let bytes = std::fs::read(&path).expect("sample readable");
    let mut engine = Engine::new().expect("builtin registry");
    engine.load_slxy(&bytes).expect("animated-field loads");

    /// Summed ABSOLUTE Y over every displayed point.
    ///
    /// Absolute on purpose: the ripple is symmetric about zero, so a plain
    /// sum cancels to roughly nothing at every frame and would call a
    /// moving surface static. (It did, the first time this was written.)
    fn height_signature(engine: &mut Engine) -> f64 {
        for _ in 0..8 {
            if engine.cook(&mut || true).is_empty() {
                break;
            }
        }
        engine
            .display_geometries()
            .iter()
            .flat_map(|(_, set, _)| set.meshes.iter())
            .flat_map(|m| m.positions.iter())
            .map(|p| f64::from(p[1]).abs())
            .sum()
    }

    let at_start = height_signature(&mut engine);
    engine
        .apply(solarxy_graph::engine::Command::SetFrame { frame: 30 })
        .expect("seek");
    let at_thirty = height_signature(&mut engine);
    assert!(
        (at_start - at_thirty).abs() > 1e-3,
        "the surface did not move between frame 1 and frame 30 \
         ({at_start} vs {at_thirty}); the sample's $T reference is gone"
    );

    // And returning to the start reproduces the start exactly: the clock is
    // a fixed step, so a frame is a frame however you arrive at it.
    engine
        .apply(solarxy_graph::engine::Command::SetFrame { frame: 1 })
        .expect("seek back");
    let back = height_signature(&mut engine);
    assert!(
        (at_start - back).abs() < 1e-6,
        "frame 1 is not reproducible ({at_start} vs {back})"
    );
}

/// The animated sample must carry the runtime settings it was authored
/// with, or it opens on the 1-240 default and the animation runs at the
/// wrong length.
#[test]
fn the_animated_sample_saves_its_runtime_settings() {
    let path = samples_dir().join("animated-field.slxy");
    if !path.is_file() {
        return;
    }
    let bytes = std::fs::read(&path).expect("sample readable");
    let mut engine = Engine::new().expect("builtin registry");
    engine.load_slxy(&bytes).expect("animated-field loads");

    let clock = engine.clock();
    assert_eq!(clock.frame_range, (1, 240));
    assert!((clock.fps - 24.0).abs() < f64::EPSILON);
    assert!(clock.autoplay, "a published scene should start playing");
    // Session state is NOT saved: the editor opens stopped whatever the
    // autoplay flag says.
    assert!(!clock.playing, "loading must never start the clock");
}

/// The instancing sample must actually instance.
///
/// The sweep above proves every scene loads and cooks, and a scatter
/// collapsed to a single copy would pass it: the geometry is still there,
/// the nodes still cook clean, and only the count is wrong. That is exactly
/// the failure the placement list can have, and this scene is the one that
/// exercises it, because it feeds its copies through a merge.
#[test]
fn the_instancing_sample_keeps_its_copies_as_placements() {
    let path = samples_dir().join("copy-and-scatter.slxy");
    if !path.is_file() {
        eprintln!("skipping: no copy-and-scatter.slxy at {}", path.display());
        return;
    }
    let bytes = std::fs::read(&path).expect("sample readable");
    let mut engine = Engine::new().expect("builtin registry");
    engine.load_slxy(&bytes).expect("copy-and-scatter loads");
    for _ in 0..8 {
        if engine.cook(&mut || true).is_empty() {
            break;
        }
    }

    let displayed = engine.display_geometries();
    assert!(!displayed.is_empty(), "the sample displays geometry");
    let placements: Vec<usize> = displayed
        .iter()
        .flat_map(|(_, set, _)| {
            set.meshes
                .iter()
                .map(solarxy_kernel::KernelMesh::instance_count)
        })
        .collect();
    assert!(
        placements.iter().any(|n| *n > 100),
        "the scattered copies reach the display as placements, not as one \
         collapsed copy: {placements:?}"
    );
}

/// Every bundled sample lights at the brightness it was authored at.
///
/// The intensity rescale moved four light types by a factor of three and
/// migrates stored numbers automatically, so most samples needed nothing.
/// One did: `animated-field.slxy` drives its key light from an expression,
/// which is the single case the migration cannot rewrite, so that file was
/// corrected by hand and stamped past the migration. A hand-edited binary
/// with no test is how a sample quietly goes dim, and the load sweep above
/// would not notice: it checks for warnings, and the corrected file has
/// none precisely because it no longer migrates.
#[test]
fn the_animated_sample_lights_at_the_rescaled_brightness() {
    use solarxy_core::scene::{LightKind, SceneOp};

    let path = samples_dir().join("animated-field.slxy");
    if !path.is_file() {
        eprintln!("skipping: no animated-field.slxy at {}", path.display());
        return;
    }
    let bytes = std::fs::read(&path).expect("sample readable");
    let mut engine = Engine::new().expect("builtin registry");
    engine.load_slxy(&bytes).expect("animated-field loads");
    for _ in 0..8 {
        if engine.cook(&mut || true).is_empty() {
            break;
        }
    }

    let lights = engine
        .take_scene_delta()
        .ops
        .into_iter()
        .find_map(|op| match op {
            SceneOp::SetLights { lights } => Some(lights),
            _ => None,
        })
        .expect("the sample has lights");

    let key = lights
        .iter()
        .find(|l| l.kind == LightKind::Directional)
        .expect("the sample has a directional key light");
    // The expression is `4.5 + sin($T * 1.1) * 1.35`, which is the original
    // `1.5 + sin($T * 1.1) * 0.45` with every term tripled. At the first
    // frame the sine term is near zero, so the key sits near its 4.5 base.
    assert!(
        (key.intensity - 4.5).abs() < 0.2,
        "the key light resolved to {}, not the rescaled base of 4.5; the \
         sample's expression was not tripled with the rest of the release",
        key.intensity
    );

    // The fill folds into the hemisphere rows and never saw the
    // multiplier, so it must NOT have moved.
    let fill = lights
        .iter()
        .find(|l| l.kind == LightKind::Hemisphere)
        .expect("the sample has a hemisphere fill");
    assert!(
        (fill.intensity - 0.7).abs() < 1e-6,
        "the hemisphere fill resolved to {}, not the 0.7 it was authored \
         at; ambient and hemisphere are outside the rescale",
        fill.intensity
    );
}

/// The flagship sample's look switch must actually switch. Its index is
/// an expression on the frame, so the shaded scene (with its instanced
/// belt) shows before frame 121 and the extracted-edge schematic after;
/// the schematic path bakes, so the placement count is the sharp signal.
#[test]
fn the_orrery_switch_flips_with_the_frame() {
    let path = samples_dir().join("the-orrery.slxy");
    if !path.is_file() {
        eprintln!("skipping: no the-orrery.slxy at {}", path.display());
        return;
    }
    let bytes = std::fs::read(&path).expect("sample readable");
    let mut engine = Engine::new().expect("builtin registry");
    engine.load_slxy(&bytes).expect("the-orrery loads");

    fn max_instances(engine: &mut Engine) -> usize {
        for _ in 0..8 {
            if engine.cook(&mut || true).is_empty() {
                break;
            }
        }
        engine
            .display_geometries()
            .iter()
            .flat_map(|(_, set, _)| set.meshes.iter())
            .map(solarxy_kernel::KernelMesh::instance_count)
            .max()
            .unwrap_or(0)
    }

    let full = max_instances(&mut engine);
    assert!(
        full > 300,
        "at frame 1 the belt reaches the display as placements ({full})"
    );
    engine
        .apply(solarxy_graph::engine::Command::SetFrame { frame: 130 })
        .expect("seek");
    let schematic = max_instances(&mut engine);
    assert!(
        schematic <= 1,
        "past frame 121 the switch shows the baked edge schematic, so no \
         placements survive (saw {schematic}); the index expression is gone"
    );
}

/// The orbits are clock-driven: the planets' horizontal spread changes
/// between two frames of the full look, and frame 1 is reproducible.
#[test]
fn the_orrery_orbits_move_with_the_clock() {
    let path = samples_dir().join("the-orrery.slxy");
    if !path.is_file() {
        return;
    }
    let bytes = std::fs::read(&path).expect("sample readable");
    let mut engine = Engine::new().expect("builtin registry");
    engine.load_slxy(&bytes).expect("the-orrery loads");

    fn spread(engine: &mut Engine) -> f64 {
        for _ in 0..8 {
            if engine.cook(&mut || true).is_empty() {
                break;
            }
        }
        engine
            .display_geometries()
            .iter()
            .flat_map(|(_, set, _)| set.meshes.iter())
            .flat_map(|m| m.positions.iter())
            .map(|p| f64::from(p[0]).abs() + f64::from(p[2]).abs())
            .sum()
    }

    let at_start = spread(&mut engine);
    engine
        .apply(solarxy_graph::engine::Command::SetFrame { frame: 60 })
        .expect("seek");
    let later = spread(&mut engine);
    assert!(
        (at_start - later).abs() > 1e-3,
        "the planets did not move between frame 1 and frame 60 \
         ({at_start} vs {later})"
    );
    engine
        .apply(solarxy_graph::engine::Command::SetFrame { frame: 1 })
        .expect("seek back");
    let back = spread(&mut engine);
    assert!(
        (at_start - back).abs() < 1e-6,
        "frame 1 is not reproducible ({at_start} vs {back})"
    );
}

/// The belt wrangle writes the colour lane, and it must reach the display:
/// vertex colour is the sample's proof that a wrangle ran at all.
#[test]
fn the_orrery_paints_the_belt() {
    use solarxy_kernel::AttributeDomain;

    let path = samples_dir().join("the-orrery.slxy");
    if !path.is_file() {
        return;
    }
    let bytes = std::fs::read(&path).expect("sample readable");
    let mut engine = Engine::new().expect("builtin registry");
    engine.load_slxy(&bytes).expect("the-orrery loads");
    for _ in 0..8 {
        if engine.cook(&mut || true).is_empty() {
            break;
        }
    }

    // The wrangle's @Cd is sugar for the engine's canonical "color" lane,
    // which is what the viewport displays and what must reach the output.
    let displayed = engine.display_geometries();
    let painted = displayed
        .iter()
        .flat_map(|(_, set, _)| set.meshes.iter())
        .any(|m| {
            m.domain_attributes(AttributeDomain::Point)
                .get("color")
                .is_some()
        });
    assert!(
        painted,
        "no displayed mesh carries the colour lane the ring wrangle writes"
    );
}
