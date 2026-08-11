//! What can go wrong, as a type rather than as a string.
//!
//! The variants are the failure *classes* a build system branches on, not the
//! places in the code that noticed. That is why "the scene would not parse" and
//! "the model would not parse" are one variant while "a node failed to cook"
//! is its own: a pipeline retries the second and gives up on the first, and it
//! can only do that if the distinction survives to the exit code.
//!
//! Library convention: `thiserror`, never `anyhow`. The command-line wrapper
//! converts.

use std::path::PathBuf;

/// A render that did not produce an image.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the input path {0} does not exist")]
    InputMissing(PathBuf),

    #[error("{path} could not be read: {source}")]
    InputUnreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} could not be loaded: {message}")]
    InputInvalid { path: PathBuf, message: String },

    #[error("{path} is not a format this renders: expected a scene or a model file")]
    InputUnsupported { path: PathBuf },

    /// A node reported an error while cooking. The render stops rather than
    /// rendering the stale geometry beside it, because a picture of a scene
    /// that failed to build is worse than no picture: it exits zero.
    #[error("the scene failed to cook: {0}")]
    Cook(String),

    #[error("the scene has no render node, and none was named")]
    NoRenderNode,

    #[error("the scene has {0} render nodes; name the one to use")]
    AmbiguousRenderNode(usize),

    #[error("{0}")]
    RenderNode(String),

    #[error("no GPU adapter is available")]
    NoAdapter,

    #[error("the GPU device could not be created: {0}")]
    Device(String),

    #[error("the GPU device was lost during the render")]
    DeviceLost,

    #[error("the render was cancelled")]
    Cancelled,

    #[error("the image could not be encoded: {0}")]
    Encode(#[from] solarxy_formats::FormatsError),

    #[error("{path} could not be written: {source}")]
    OutputUnwritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl RenderError {
    /// Which step of a render this ended.
    ///
    /// Coarser than the variant on purpose: a progress sink wants to close its
    /// line with the name of the thing that was happening, not with a failure
    /// class. The two group differently, which is why this is a mapping rather
    /// than a derive.
    #[must_use]
    pub fn stage(&self) -> &'static str {
        match self {
            Self::InputMissing(_)
            | Self::InputUnreadable { .. }
            | Self::InputInvalid { .. }
            | Self::InputUnsupported { .. } => "loading",
            Self::Cook(_) => "cooking",
            Self::NoRenderNode | Self::AmbiguousRenderNode(_) | Self::RenderNode(_) => {
                "resolving the render"
            }
            Self::NoAdapter | Self::Device(_) => "starting the GPU",
            Self::DeviceLost => "drawing",
            Self::Cancelled => "cancelled",
            Self::Encode(_) | Self::OutputUnwritable { .. } => "writing",
        }
    }
}
