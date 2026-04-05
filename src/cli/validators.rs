use std::path::PathBuf;

use solarxy::SUPPORTED_EXTENSIONS;

pub fn is_valid_model_path(path: &str) -> Result<PathBuf, String> {
    let path_buf = PathBuf::from(path);

    if !path_buf.exists() {
        return Err(format!("Model file does not exist: {}", path));
    }

    if !path_buf.is_file() {
        return Err(format!("Path is not a file: {}", path));
    }

    match path_buf.extension().and_then(|ext| ext.to_str()) {
        Some(ext)
            if SUPPORTED_EXTENSIONS
                .iter()
                .any(|s| ext.eq_ignore_ascii_case(s)) =>
        {
            path_buf
                .canonicalize()
                .map_err(|e| format!("Failed to resolve path: {}", e))
        }
        Some(ext) => Err(format!(
            "Invalid file extension '.{}', expected '.obj', '.stl', '.ply', '.gltf', or '.glb'",
            ext
        )),
        None => Err(String::from(
            "File has no extension, expected '.obj', '.stl', '.ply', '.gltf', or '.glb'",
        )),
    }
}
