//! Deterministic meshes the crate's own tests build hierarchies over.
//!
//! Generated rather than loaded: this crate has no filesystem access by
//! design, and a fixture on disk would be a second thing to keep in step with
//! the tests that read it.

/// A `n * n` quad grid on the XY plane, two triangles per quad.
///
/// Perfectly regular, which is the point: it makes leaf sizes and tree depth
/// predictable enough to assert on.
pub fn grid(n: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for y in 0..=n {
        for x in 0..=n {
            positions.push([x as f32, y as f32, 0.0]);
        }
    }
    let stride = n + 1;
    for y in 0..n {
        for x in 0..n {
            let a = y * stride + x;
            indices.extend_from_slice(&[a, a + 1, a + stride]);
            indices.extend_from_slice(&[a + 1, a + stride + 1, a + stride]);
        }
    }
    (positions, indices)
}

/// A UV sphere of unit radius: `width * height * 2` triangles, with the
/// degenerate pole quads left in exactly as a real generator emits them.
///
/// Curved and closed, so unlike the grid it exercises rays that enter and
/// leave, box tests that reject at an angle, and leaves whose bounds overlap.
pub fn sphere(width: u32, height: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::with_capacity(((width + 1) * (height + 1)) as usize);
    for y in 0..=height {
        let v = f32::from(u16::try_from(y).unwrap_or(u16::MAX)) / height as f32;
        let phi = v * std::f32::consts::PI;
        for x in 0..=width {
            let u = f32::from(u16::try_from(x).unwrap_or(u16::MAX)) / width as f32;
            let theta = u * std::f32::consts::TAU;
            positions.push([phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()]);
        }
    }

    let stride = width + 1;
    let mut indices = Vec::with_capacity((width * height * 6) as usize);
    for y in 0..height {
        for x in 0..width {
            let a = y * stride + x;
            indices.extend_from_slice(&[a, a + stride, a + 1]);
            indices.extend_from_slice(&[a + 1, a + stride, a + stride + 1]);
        }
    }
    (positions, indices)
}
