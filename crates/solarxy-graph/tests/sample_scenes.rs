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
