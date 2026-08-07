//! The shared parity corpus: deterministic meshes and the ray set that both
//! sides of every traversal comparison draw from.
//!
//! Three implementations of the same intersection have to agree:
//! `solarxy_core::raycast`, this crate's traversal, and the WGSL kernel. Two of
//! those comparisons live in different crates, so if each built its own corpus
//! they would agree by copy-paste rather than by construction, and a fix to the
//! ray policy would land in one and not the other. They draw from here instead.
//!
//! Generated rather than loaded: this crate has no filesystem access by design,
//! a fixture on disk would be a second thing to keep in step, and a generated
//! mesh gives the same triangle count on every machine that reproduces a
//! measurement.
//!
//! Nothing here is referenced by the shipped shells, so it costs no payload.

/// Deterministic unit-interval draws. xorshift32, because the corpus has to be
/// identical on every machine that runs the gate.
pub struct Rng(u32);

impl Rng {
    /// Seeds the generator. Zero is a fixed point of xorshift and yields a
    /// constant stream, so it is not a useful seed.
    #[must_use]
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    /// The next draw in `[0, 1]`.
    pub fn next_unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32
    }

    /// The next draw scaled into `[lo, hi]`.
    pub fn next_range(&mut self, lo: f32, hi: f32) -> f32 {
        self.next_unit().mul_add(hi - lo, lo)
    }
}

/// One ray of the corpus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorpusRay {
    /// Which draw produced this ray. Degenerate draws are skipped rather than
    /// resampled, so this is not the position in the returned slice, and it is
    /// what a failure message should name: it identifies the same ray on every
    /// machine and in every one of the three implementations.
    pub index: u32,
    pub origin: [f32; 3],
    /// Unit length.
    pub direction: [f32; 3],
}

/// `count` draws of a ray corpus aimed at geometry near the origin.
///
/// Half the rays are aimed into a 2.4-unit box around the origin and half are
/// free. A fully random direction from a 6-unit cube finds a unit sphere only
/// about one time in sixteen, which would spend most of the corpus proving that
/// a ray into empty space misses. The aimed half is what drives the traversal
/// down to the leaves; the free half is what keeps the box rejections honest.
///
/// Draws whose direction is shorter than `1e-3` are dropped rather than
/// resampled, so the stream stays a pure function of the seed.
#[must_use]
pub fn rays(seed: u32, count: u32) -> Vec<CorpusRay> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(count as usize);

    for index in 0..count {
        let origin = [
            rng.next_range(-3.0, 3.0),
            rng.next_range(-3.0, 3.0),
            rng.next_range(-3.0, 3.0),
        ];
        let raw = if index % 2 == 0 {
            [
                rng.next_range(-1.2, 1.2) - origin[0],
                rng.next_range(-1.2, 1.2) - origin[1],
                rng.next_range(-1.2, 1.2) - origin[2],
            ]
        } else {
            [
                rng.next_range(-1.0, 1.0),
                rng.next_range(-1.0, 1.0),
                rng.next_range(-1.0, 1.0),
            ]
        };

        // Length and normalization are spelled out as `magnitude` then multiply
        // by the reciprocal, which is what `cgmath` does. The corpus predates
        // this module inside the parity test and used `cgmath` there; dividing
        // instead would shift every direction by an ulp and quietly change
        // which grazing rays hit.
        let len = raw[0]
            .mul_add(raw[0], raw[1].mul_add(raw[1], raw[2] * raw[2]))
            .sqrt();
        if len < 1e-3 {
            continue;
        }
        let inv = 1.0 / len;
        out.push(CorpusRay {
            index,
            origin,
            direction: [raw[0] * inv, raw[1] * inv, raw[2] * inv],
        });
    }

    out
}

/// A `n * n` quad grid on the XY plane, two triangles per quad, one unit apart.
///
/// Perfectly regular, which is the point: it makes leaf sizes and tree depth
/// predictable enough to assert on.
#[must_use]
pub fn grid(n: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
    grid_spaced(n, 1.0, 0.0)
}

/// A `n * n` quad grid centred on the origin with `step` between vertices.
///
/// Every triangle shares one plane, so the root box is degenerate on an axis
/// and the slab test divides by an infinite reciprocal on every query. That is
/// the case which separates a robust slab test from a plausible one.
#[must_use]
pub fn coplanar_grid(n: u32, step: f32) -> (Vec<[f32; 3]>, Vec<u32>) {
    grid_spaced(n, step, -(n as f32 * step) / 2.0)
}

fn grid_spaced(n: u32, step: f32, origin: f32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::with_capacity(((n + 1) * (n + 1)) as usize);
    let mut indices = Vec::with_capacity((n * n * 6) as usize);
    for y in 0..=n {
        for x in 0..=n {
            // Plain multiply-then-add rather than `mul_add`: the coplanar grid
            // has to be the same mesh the parity corpus measured before this
            // module existed, and fusing the two operations rounds once where
            // the original rounded twice.
            positions.push([x as f32 * step + origin, y as f32 * step + origin, 0.0]);
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
#[must_use]
pub fn sphere(width: u32, height: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::with_capacity(((width + 1) * (height + 1)) as usize);
    for y in 0..=height {
        let phi = (y as f32 / height as f32) * std::f32::consts::PI;
        for x in 0..=width {
            let theta = (x as f32 / width as f32) * std::f32::consts::TAU;
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

#[cfg(test)]
mod tests {
    use super::{coplanar_grid, grid, rays, sphere};

    #[test]
    fn the_ray_stream_is_a_pure_function_of_the_seed() {
        assert_eq!(rays(0x9E37_79B9, 256), rays(0x9E37_79B9, 256));
        assert_ne!(rays(0x9E37_79B9, 256), rays(0x1357_9BDF, 256));
    }

    #[test]
    fn every_corpus_direction_is_unit_length() {
        for ray in rays(0x9E37_79B9, 512) {
            let d = ray.direction;
            let len2 = d[0].mul_add(d[0], d[1].mul_add(d[1], d[2] * d[2]));
            assert!((len2 - 1.0).abs() < 1e-5, "ray {} is not unit", ray.index);
        }
    }

    #[test]
    fn the_generated_meshes_have_the_triangle_counts_they_advertise() {
        let (_, indices) = sphere(48, 24);
        assert_eq!(indices.len(), 48 * 24 * 6);
        let (_, indices) = grid(30);
        assert_eq!(indices.len(), 30 * 30 * 6);
        let (positions, _) = coplanar_grid(30, 0.1);
        assert!(positions.iter().all(|p| p[2] == 0.0));
    }
}
