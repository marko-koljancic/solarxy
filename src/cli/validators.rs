use std::path::PathBuf;

pub fn is_valid_obj_model_path(path: &str) -> Result<PathBuf, String> {
    let path_buf = PathBuf::from(path);
    if !path_buf.exists() {
        return Err(String::from("The specified model file does not exist"));
    }
    if path_buf.extension().and_then(|ext| ext.to_str()) == Some("obj") {
        Ok(path_buf)
    } else {
        Err(String::from("The model file must have a .obj extension"))
    }
}
