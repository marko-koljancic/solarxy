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
