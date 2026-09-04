//! Regression test for the materials-over-the-worker path: a
//! textured OBJ (MTL + PNG resolved through the asset table) must carry
//! its material, including decoded texture pixels, through parse, the
//! transfer codec, and the cooked-geometry lowering.

use solarxy_graph::assets::AssetTable;
use solarxy_graph::cook::ImportOptions;
use solarxy_graph::nodes::parse_model;
use solarxy_kernel::transfer;

fn knot_files() -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../res/models/knot");
    let obj = std::fs::read(format!("{base}/knot.obj")).ok()?;
    let mtl = std::fs::read(format!("{base}/knot.mtl")).ok()?;
    let png = std::fs::read(format!("{base}/diffuse.png")).ok()?;
    Some((obj, mtl, png))
}

#[test]
fn textured_obj_materials_survive_parse_transfer_and_cook() {
    let Some((obj, mtl, png)) = knot_files() else {
        eprintln!("knot test model absent; skipping");
        return;
    };

    let mut table = AssetTable::new();
    table.stage("model.obj".to_string(), String::new(), obj.clone());
    table.stage("knot.mtl".to_string(), String::new(), mtl);
    table.stage("diffuse.png".to_string(), String::new(), png);

    let options = ImportOptions {
        scale: 1.0,
        center_to_origin: false,
        recompute_normals: None,
        preserve_materials: None,
        vertex_colors: None,
    };
    let set = parse_model("obj", &obj, "model.obj", &table, &options).expect("parse");

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

/// Regression for the multi-file glTF path: the import worker rebuilds
/// exactly this flow (stage the shipped files into a table, resolve the
/// external `.bin` by trailing basename). A staged buffer resolves even
/// when the URI carries a subpath; a missing one fails with the actionable
/// error naming the companion file.
#[test]
fn multi_file_gltf_resolves_its_external_buffer_through_the_table() {
    let positions: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},
            "buffers":[{{"uri":"buffers/FlightHelmet.bin","byteLength":{len}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{len}}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],
            "nodes":[{{"mesh":0}}],
            "scenes":[{{"nodes":[0]}}]}}"#,
        len = positions.len()
    );
    let options = ImportOptions {
        scale: 1.0,
        center_to_origin: false,
        recompute_normals: None,
        preserve_materials: None,
        vertex_colors: None,
    };

    // Staged under the bare basename (how every entry path stages files),
    // requested with a subpath URI: the resolver matches the basename.
    let mut table = AssetTable::new();
    table.stage(
        "FlightHelmet.gltf".to_string(),
        String::new(),
        json.clone().into_bytes(),
    );
    table.stage("FlightHelmet.bin".to_string(), String::new(), positions);
    let set = parse_model(
        "gltf",
        json.as_bytes(),
        "FlightHelmet.gltf",
        &table,
        &options,
    )
    .expect("external buffer resolves through the table");
    assert_eq!(set.meshes.len(), 1);
    assert_eq!(set.meshes[0].positions.len(), 3);

    // Without the buffer staged, the error names the missing companion.
    let mut lone = AssetTable::new();
    lone.stage(
        "FlightHelmet.gltf".to_string(),
        String::new(),
        json.clone().into_bytes(),
    );
    let err = parse_model(
        "gltf",
        json.as_bytes(),
        "FlightHelmet.gltf",
        &lone,
        &options,
    )
    .expect_err("missing buffer must fail the parse");
    let msg = err.to_string();
    // The message names the companion by its referenced URI (subpath and
    // all) and tells the user what to do about it.
    assert!(
        msg.contains("missing external asset 'buffers/FlightHelmet.bin'"),
        "unexpected error text: {msg}"
    );
    assert!(
        msg.contains("import or place it alongside the model"),
        "unexpected error text: {msg}"
    );
}
