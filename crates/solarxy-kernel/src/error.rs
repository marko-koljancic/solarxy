//! Kernel error type. Library convention: `thiserror` here, `anyhow` only
//! in binary shells.

/// Errors produced by kernel operations.
///
/// Deliberately small: generator parameters are validated upstream by the
/// graph's param resolver (hard ranges), so the kernel only fails on inputs
/// that no resolver can rule out, such as a caller-supplied singular matrix.
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    /// The transform matrix has no inverse, so the inverse-transpose normal
    /// matrix cannot be derived. Cannot occur through the transform node
    /// (its scale hard range excludes zero) but can with an arbitrary
    /// caller-supplied matrix.
    #[error("transform matrix is singular; cannot derive the normal matrix")]
    SingularTransform,
}
