//! Regression test for the phase-6 materials-over-the-worker path: a
//! textured OBJ (MTL + PNG resolved through the asset table) must carry
//! its material, including decoded texture pixels, through parse, the
//! transfer codec, and the cooked-geometry lowering.

use solarxy_graph::assets::AssetTable;
use solarxy_graph::cook::ImportOptions;
use solarxy_graph::nodes::parse_model;
use solarxy_kernel::transfer;

fn frog_files() -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../res/models/frog");
    let obj = std::fs::read(format!("{base}/ooz3d-export-model-20260329-181053.obj")).ok()?;
    let mtl = std::fs::read(format!("{base}/ooz3d-export-material-20260329-181053.mtl")).ok()?;
    let png = std::fs::read(format!("{base}/diffuse.png")).ok()?;
    Some((obj, mtl, png))
}

#[test]
fn textured_obj_materials_survive_parse_transfer_and_cook() {
    let Some((obj, mtl, png)) = frog_files() else {
        eprintln!("frog test model absent; skipping");
        return;
    };
    // The shipped export never binds its material; insert the usemtl the
    // test needs (the file's own gap, not a loader bug).
    let obj_text = String::from_utf8_lossy(&obj);
    let needle = "mtllib ooz3d-export-material-20260329-181053.mtl\n";
    let obj_fixed: String = obj_text.replacen(needle, &format!("{needle}usemtl Material1\n"), 1);

    let mut table = AssetTable::new();
    table.stage(
        "model.obj".to_string(),
        String::new(),
        obj_fixed.clone().into_bytes(),
    );
    table.stage(
        "ooz3d-export-material-20260329-181053.mtl".to_string(),
        String::new(),
        mtl,
    );
    table.stage("diffuse.png".to_string(), String::new(), png);

    let options = ImportOptions {
        scale: 1.0,
        center_to_origin: false,
        recompute_normals: None,
        preserve_materials: None,
    };
    let set =
        parse_model("obj", obj_fixed.as_bytes(), "model.obj", &table, &options).expect("parse");

    // Parse: material present, texture decoded, meshes bound to it.
    assert_eq!(set.materials.len(), 1, "one MTL material");
    let tex = set.materials[0]
        .diffuse_texture_data
        .as_ref()
        .expect("diffuse texture decoded at parse");
    assert_eq!((tex.width, tex.height), (1024, 1024));
    assert!(
        set.meshes.iter().any(|m| m.material_index == Some(0)),
        "meshes bind the material"
    );

    // Transfer codec: everything survives the worker boundary.
    let back = transfer::unpack(&transfer::pack(&set)).expect("transfer");
    assert_eq!(back.materials.len(), 1);
    let back_tex = back.materials[0]
        .diffuse_texture_data
        .as_ref()
        .expect("texture survives the codec");
    assert_eq!(back_tex.pixels.len(), tex.pixels.len());
    assert!(back.meshes.iter().any(|m| m.material_index == Some(0)));

    // Cooked lowering: the scene delta payload still carries materials.
    let cooked = back.to_cooked();
    assert_eq!(cooked.materials.len(), 1);
    assert!(cooked.materials[0].diffuse_texture_data.is_some());
    assert!(cooked.meshes.iter().any(|m| m.material_index == Some(0)));
}
