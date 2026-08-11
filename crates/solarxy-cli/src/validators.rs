use std::path::PathBuf;

use solarxy_core::SUPPORTED_EXTENSIONS;

/// The extensions this accepts, phrased for a message.
///
/// Derived from the list rather than written beside it. The two used to be
/// separate, so a format added to the list left the error still naming the old
/// five.
fn expected() -> String {
    let mut parts: Vec<String> = SUPPORTED_EXTENSIONS
        .iter()
        .map(|e| format!("'.{e}'"))
        .collect();
    if let Some(last) = parts.pop() {
        format!("{}, or {last}", parts.join(", "))
    } else {
        String::new()
    }
}

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
            "Invalid file extension '.{ext}', expected {}",
            expected()
        )),
        None => Err(format!("File has no extension, expected {}", expected())),
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

/// What the render subcommand accepts: a scene file, or any model the loaders
/// read.
///
/// A separate validator rather than a wider `SUPPORTED_EXTENSIONS`, because
/// that list is what the analyze surface accepts and the analyzer cannot read a
/// scene file. Widening it would make `--mode analyze -m scene.slxy` parse and
/// then fail somewhere less helpful.
///
/// # Errors
/// The path not existing, not being a file, or not being a kind this renders.
pub fn is_valid_render_input(path: &str) -> Result<PathBuf, String> {
    // Kind only. Whether the file is there, and whether it parses, is the
    // renderer's answer: those are input failures with their own exit code, and
    // deciding them here would report them as usage errors instead.
    let path_buf = PathBuf::from(path);
    match path_buf.extension().and_then(|e| e.to_str()) {
        Some(ext)
            if ext.eq_ignore_ascii_case("slxy")
                || SUPPORTED_EXTENSIONS
                    .iter()
                    .any(|s| ext.eq_ignore_ascii_case(s)) =>
        {
            Ok(path_buf)
        }
        Some(ext) => Err(format!(
            "Cannot render '.{ext}', expected '.slxy' or {}",
            expected()
        )),
        None => Err(format!(
            "File has no extension, expected '.slxy' or {}",
            expected()
        )),
    }
}
