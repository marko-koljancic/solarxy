use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("surface acquisition failed")]
    SurfaceLost(#[source] wgpu::SurfaceError),

    #[error("device was lost")]
    DeviceLost,

    #[error("failed to create default texture '{name}'")]
    DefaultTexture { name: &'static str },

    #[error("shader compilation failed for '{shader}': {message}")]
    ShaderCompile {
        shader: &'static str,
        message: String,
    },

    #[error("image decode failed for {path}")]
    ImageDecode {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },

    #[error("model load failed: {0}")]
    ModelLoad(String),

    #[error("ibl initialization failed: {0}")]
    IblInit(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type RendererResult<T> = Result<T, RendererError>;
