use std::path::PathBuf;

/// Validates that the given path points to a valid OBJ model file.
///
/// This function performs comprehensive validation of a file path to ensure it:
/// - Exists on the filesystem
/// - Is a file (not a directory or other file type)
/// - Has the `.obj` extension (case-insensitive)
///
/// If validation succeeds, the path is canonicalized to resolve any relative paths,
/// symlinks, or `.` and `..` components into an absolute path.
///
/// # Arguments
///
/// * `path` - A string slice representing the path to validate
///
/// # Returns
///
/// * `Ok(PathBuf)` - A canonicalized absolute path to the OBJ file
/// * `Err(String)` - A descriptive error message if validation fails
///
/// # Errors
///
/// This function will return an error if:
/// - The path does not exist
/// - The path exists but is not a file (e.g., it's a directory)
/// - The file does not have an `.obj` extension
/// - The file has no extension
/// - Path canonicalization fails (rare, but possible with permission issues)
pub fn is_valid_obj_model_path(path: &str) -> Result<PathBuf, String> {
    let path_buf = PathBuf::from(path);

    if !path_buf.exists() {
        return Err(format!("Model file does not exist: {}", path));
    }

    if !path_buf.is_file() {
        return Err(format!("Path is not a file: {}", path));
    }

    match path_buf.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("obj") => path_buf
            .canonicalize()
            .map_err(|e| format!("Failed to resolve path: {}", e)),
        Some(ext) => Err(format!("Invalid file extension '.{}', expected '.obj'", ext)),
        None => Err(String::from("File has no extension, expected '.obj'")),
    }
}
