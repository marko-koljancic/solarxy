use std::path::PathBuf;

use solarxy_core::SUPPORTED_EXTENSIONS;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_path_nonexistent_file() {
        let result = is_valid_model_path("/nonexistent/model.obj");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn valid_path_unsupported_extension() {
        let result = is_valid_model_path("Cargo.toml");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid file extension"));
    }

    #[test]
    fn valid_path_directory_not_file() {
        let result = is_valid_model_path("src");
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("not a file"),
            "should reject directories"
        );
    }
}
