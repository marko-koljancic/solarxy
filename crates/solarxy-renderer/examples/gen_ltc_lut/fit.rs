//! The LTC fitting maths, shared by the bake and by the test that checks it.
//!
//! Lives beside the example rather than inside `solarxy-renderer` because
//! none of it ships: the renderer only ever reads the baked table. The
//! verification test reaches it with `#[path]` for the same reason, so
//! there is exactly one definition of the BRDF the table claims to
//! approximate. A second copy in the test would let the fit and its own
//! oracle drift into agreeing about the wrong thing.
//!
//! Two consumers means each compiles the whole module and uses a subset of
//! it: the bake needs the search, the test needs only the evaluation. Hence
//! the blanket dead-code allow, which is about the sharing rather than
//! about anything here being unused.

#![allow(dead_code)]

/// Table edge, and the contract with `solarxy_renderer::ltc`.
pub const N: usize = 64;
/// Below this the lobe is a delta and the fit stops being meaningful.
pub const MIN_ALPHA: f64 = 1.0e-5;
/// Integration samples per axis, so `NSAMPLE^2` directions per estimate.
///
/// Raising this to 64 was measured to change the fitted table by less than
/// its distance to the published reference, so the residual is a different
/// local optimum rather than sampling noise, and 32 is where the returns
/// stop.
pub const NSAMPLE: usize = 32;

/// The view direction for a table row: `v` indexes `sqrt(1 - dot(N, V))`.
#[must_use]
pub fn view_for_row(t: usize) -> Vec3 {
    let x = t as f64 / (N - 1) as f64;
    let cos_theta = 1.0 - x * x;
    // 1.57 rather than pi/2: a grazing view is a degenerate fit.
    let theta = cos_theta.acos().min(1.57);
    [theta.sin(), 0.0, theta.cos()]
}

/// The GGX alpha for a table column: `u` indexes perceptual roughness.
#[must_use]
pub fn alpha_for_column(a: usize) -> f64 {
    let roughness = a as f64 / (N - 1) as f64;
    (roughness * roughness).max(MIN_ALPHA)
}

pub type Vec3 = [f64; 3];
/// Row-major: `m[row][col]`. Written out rather than pulled from `cgmath`
/// so the matrix convention is visible at every use, which is where LTC
/// implementations usually go wrong.
pub type Mat3 = [[f64; 3]; 3];

pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

pub fn normalize(a: Vec3) -> Vec3 {
    let l = length(a);
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

pub fn mul(m: &Mat3, v: Vec3) -> Vec3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

pub fn det(m: &Mat3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

pub fn inverse(m: &Mat3) -> Mat3 {
    let d = det(m);
    // A singular M means the fit collapsed the lobe to a plane. Returning
    // the identity keeps the search alive; the error term rejects it.
    if d.abs() < 1.0e-30 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let inv_d = 1.0 / d;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_d,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_d,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_d,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_d,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_d,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_d,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_d,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_d,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_d,
        ],
    ]
}

/// GGX with the Smith height-correlated masking-shadowing term, evaluated
/// in the shading frame where the normal is `+z`.
///
/// Returns `(brdf * cos(L), pdf)`. The cosine is folded in because the LTC
/// approximates the whole `brdf * cos` lobe, not the BRDF alone.
pub fn ggx_eval(v: Vec3, l: Vec3, alpha: f64) -> (f64, f64) {
    if v[2] <= 0.0 {
        return (0.0, 0.0);
    }
    let lambda = |z: f64| -> f64 {
        if z >= 1.0 {
            return 0.0;
        }
        let tan_theta = (1.0 - z * z).sqrt() / z;
        let a = 1.0 / (alpha * tan_theta);
        0.5 * (-1.0 + (1.0 + 1.0 / (a * a)).sqrt())
    };
    let lambda_v = lambda(v[2]);

    let g2 = if l[2] <= 0.0 {
        0.0
    } else {
        1.0 / (1.0 + lambda_v + lambda(l[2]))
    };

    let h = normalize([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
    if h[2] <= 0.0 {
        return (0.0, 0.0);
    }
    let slope_x = h[0] / h[2];
    let slope_y = h[1] / h[2];
    let mut d = 1.0 / (1.0 + (slope_x * slope_x + slope_y * slope_y) / (alpha * alpha));
    d *= d;
    d /= std::f64::consts::PI * alpha * alpha * h[2] * h[2] * h[2] * h[2];

    let vh = dot(v, h);
    if vh <= 0.0 {
        return (0.0, 0.0);
    }
    let pdf = (d * h[2] / (4.0 * vh)).abs();
    (d * g2 / (4.0 * v[2]), pdf)
}

/// Samples a GGX half-vector and reflects the view about it.
pub fn ggx_sample(v: Vec3, alpha: f64, u1: f64, u2: f64) -> Vec3 {
    let phi = 2.0 * std::f64::consts::PI * u1;
    let r = alpha * (u2 / (1.0 - u2)).sqrt();
    let h = normalize([r * phi.cos(), r * phi.sin(), 1.0]);
    let vh = dot(h, v);
    [
        -v[0] + 2.0 * h[0] * vh,
        -v[1] + 2.0 * h[1] * vh,
        -v[2] + 2.0 * h[2] * vh,
    ]
}

/// One fitted lobe: a clamped cosine pushed through `M`.
pub struct Ltc {
    pub magnitude: f64,
    pub fresnel: f64,
    /// The three free parameters. `M` is the frame times
    /// `[[m11, 0, m13], [0, m22, 0], [0, 0, 1]]`, which is the isotropic
    /// case: a fourth parameter would only tilt the lobe out of the
    /// incidence plane, and an isotropic BRDF never does that.
    pub m11: f64,
    pub m22: f64,
    pub m13: f64,
    /// The frame the lobe is fitted in, aligned to the average direction.
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
    pub m: Mat3,
    pub inv_m: Mat3,
    pub det_m: f64,
}

impl Ltc {
    pub fn new() -> Self {
        let mut ltc = Ltc {
            magnitude: 1.0,
            fresnel: 1.0,
            m11: 1.0,
            m22: 1.0,
            m13: 0.0,
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
            m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            inv_m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            det_m: 1.0,
        };
        ltc.update();
        ltc
    }

    /// Rebuilds `M` from the frame and the three parameters.
    ///
    /// `M = [X Y Z] * S` with the frame vectors as COLUMNS, so column 0 is
    /// `X * m11`, column 1 is `Y * m22`, and column 2 is `X * m13 + Z`.
    pub fn update(&mut self) {
        let c0 = [
            self.x[0] * self.m11,
            self.x[1] * self.m11,
            self.x[2] * self.m11,
        ];
        let c1 = [
            self.y[0] * self.m22,
            self.y[1] * self.m22,
            self.y[2] * self.m22,
        ];
        let c2 = [
            self.x[0] * self.m13 + self.z[0],
            self.x[1] * self.m13 + self.z[1],
            self.x[2] * self.m13 + self.z[2],
        ];
        self.m = [
            [c0[0], c1[0], c2[0]],
            [c0[1], c1[1], c2[1]],
            [c0[2], c1[2], c2[2]],
        ];
        self.inv_m = inverse(&self.m);
        self.det_m = det(&self.m).abs();
    }

    /// The lobe's value in direction `l`: the clamped cosine evaluated at
    /// the pre-image of `l`, divided by the transform's Jacobian.
    pub fn eval(&self, l: Vec3) -> f64 {
        let original = normalize(mul(&self.inv_m, l));
        let back = mul(&self.m, original);
        let len = length(back);
        if len <= 0.0 || self.det_m <= 0.0 {
            return 0.0;
        }
        let jacobian = self.det_m / (len * len * len);
        let d = original[2].max(0.0) / std::f64::consts::PI;
        self.magnitude * d / jacobian
    }

    /// Cosine-samples the original lobe and pushes it through `M`.
    pub fn sample(&self, u1: f64, u2: f64) -> Vec3 {
        let theta = u1.sqrt().acos();
        let phi = 2.0 * std::f64::consts::PI * u2;
        normalize(mul(
            &self.m,
            [
                theta.sin() * phi.cos(),
                theta.sin() * phi.sin(),
                theta.cos(),
            ],
        ))
    }
}

/// The lobe's total energy, its Fresnel term, and the direction it points.
///
/// These are integrated rather than fitted: only the lobe's SHAPE needs a
/// search, and starting the search from the true average direction is what
/// keeps the fit from wandering.
pub fn compute_avg_terms(v: Vec3, alpha: f64) -> (f64, f64, Vec3) {
    let mut norm = 0.0;
    let mut fresnel = 0.0;
    let mut average = [0.0, 0.0, 0.0];

    for j in 0..NSAMPLE {
        for i in 0..NSAMPLE {
            let u1 = (i as f64 + 0.5) / NSAMPLE as f64;
            let u2 = (j as f64 + 0.5) / NSAMPLE as f64;
            let l = ggx_sample(v, alpha, u1, u2);
            let (eval, pdf) = ggx_eval(v, l, alpha);
            if pdf <= 0.0 {
                continue;
            }
            let weight = eval / pdf;
            let h = normalize([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
            norm += weight;
            fresnel += weight * (1.0 - dot(v, h)).max(0.0).powi(5);
            average[0] += weight * l[0];
            average[1] += weight * l[1];
            average[2] += weight * l[2];
        }
    }

    let inv = 1.0 / (NSAMPLE * NSAMPLE) as f64;
    // The y component is zero for an isotropic BRDF; clearing it removes
    // the sampling noise that would otherwise tilt the frame.
    average[1] = 0.0;
    (norm * inv, fresnel * inv, normalize(average))
}

/// How badly the lobe matches the BRDF, as the L3 norm under multiple
/// importance sampling.
///
/// L3 rather than L2 because the cube weights the bright core of the lobe
/// far above its tail, and the core is what a highlight actually is.
pub fn compute_error(ltc: &Ltc, v: Vec3, alpha: f64) -> f64 {
    let mut error = 0.0;
    for j in 0..NSAMPLE {
        for i in 0..NSAMPLE {
            let u1 = (i as f64 + 0.5) / NSAMPLE as f64;
            let u2 = (j as f64 + 0.5) / NSAMPLE as f64;

            // Sample both distributions: either alone leaves the other's
            // tail unmeasured, which is how a fit passes its own error
            // metric while looking wrong.
            for l in [ltc.sample(u1, u2), ggx_sample(v, alpha, u1, u2)] {
                let (eval_brdf, pdf_brdf) = ggx_eval(v, l, alpha);
                let eval_ltc = ltc.eval(l);
                let pdf_ltc = if ltc.magnitude > 0.0 {
                    eval_ltc / ltc.magnitude
                } else {
                    0.0
                };
                let denom = pdf_ltc + pdf_brdf;
                if denom <= 0.0 {
                    continue;
                }
                let d = (eval_brdf - eval_ltc).abs();
                error += d * d * d / denom;
            }
        }
    }
    error / (NSAMPLE * NSAMPLE) as f64
}

/// Downhill simplex over the three lobe parameters.
///
/// Derivative-free on purpose: the error is a Monte Carlo estimate, so its
/// gradient is noise.
pub fn nelder_mead<F: FnMut(&[f64; 3]) -> f64>(
    start: [f64; 3],
    delta: f64,
    tolerance: f64,
    max_iters: usize,
    f: &mut F,
) -> [f64; 3] {
    let mut simplex = [start; 4];
    for i in 0..3 {
        simplex[i + 1][i] += delta;
    }
    let mut values = simplex.map(|p| f(&p));

    for _ in 0..max_iters {
        // Order by value: best first, worst last.
        let mut order = [0usize, 1, 2, 3];
        order.sort_by(|a, b| values[*a].total_cmp(&values[*b]));
        let (best, worst, second_worst) = (order[0], order[3], order[2]);

        if (values[worst] - values[best]).abs() < tolerance {
            break;
        }

        // Centroid of everything except the worst point.
        let mut centroid = [0.0f64; 3];
        for &i in &order[..3] {
            for k in 0..3 {
                centroid[k] += simplex[i][k] / 3.0;
            }
        }

        let step = |t: f64| -> [f64; 3] {
            std::array::from_fn(|k| centroid[k] + t * (simplex[worst][k] - centroid[k]))
        };

        let reflected = step(-1.0);
        let value = f(&reflected);
        if value < values[best] {
            // Reflection helped: try going further in the same direction.
            let expanded = step(-2.0);
            let expanded_value = f(&expanded);
            if expanded_value < value {
                simplex[worst] = expanded;
                values[worst] = expanded_value;
            } else {
                simplex[worst] = reflected;
                values[worst] = value;
            }
        } else if value < values[second_worst] {
            simplex[worst] = reflected;
            values[worst] = value;
        } else {
            let contracted = step(0.5);
            let contracted_value = f(&contracted);
            if contracted_value < values[worst] {
                simplex[worst] = contracted;
                values[worst] = contracted_value;
            } else {
                // Nothing worked: pull the whole simplex toward the best.
                let anchor = simplex[best];
                for &i in &order[1..] {
                    for (p, a) in simplex[i].iter_mut().zip(anchor) {
                        *p = a + 0.5 * (*p - a);
                    }
                    values[i] = f(&simplex[i]);
                }
            }
        }
    }

    let mut best = 0;
    for i in 1..4 {
        if values[i] < values[best] {
            best = i;
        }
    }
    simplex[best]
}
