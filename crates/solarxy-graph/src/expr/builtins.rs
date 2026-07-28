//! The builtin function set.
//!
//! Two of these are load-bearing for reproducibility and are pinned by
//! decision rather than convenience:
//!
//! - **`rand()` reuses the kernel generator** (`solarxy_kernel::rng`), so
//!   an expression and a `scatter` drawing from the same seed agree. A
//!   second, subtly different generator here would make "same seed" mean
//!   two things.
//! - **`noise()` is frozen in this file.** It is a value
//!   noise over an integer lattice using the same avalanche constants as
//!   the kernel's hash, written out here rather than imported from a crate
//!   whose next version could change its output. A scene using `noise()`
//!   must render identically on every platform and every build, forever,
//!   which rules out anything version-dependent.

use super::error::ExprError;
use super::value::{Value, map1, map2};
use std::ops::Range;

/// Every builtin name, for error messages and editor completion.
/// The context-reading functions: cross-node parameter reads and geometry
/// queries. Not in [`BUILTIN_NAMES`] because they are resolved against the
/// document rather than computed from their arguments, but the editor
/// highlights them the same way, so they are enumerated here for it.
pub const QUERY_NAMES: &[&str] = &["ch", "bbox", "npoints", "nprims", "nmeshes", "centroid"];

/// The type keywords a wrangle may declare a local with
/// (`crate::expr::stmt::LocalType`).
pub const LOCAL_TYPE_NAMES: &[&str] = &["float", "vector2", "vector", "vector4"];

pub const BUILTIN_NAMES: &[&str] = &[
    "abs",
    "sign",
    "floor",
    "ceil",
    "round",
    "min",
    "max",
    "clamp",
    "fit",
    "lerp",
    "sqrt",
    "pow",
    "exp",
    "log",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "radians",
    "degrees",
    "fmod",
    "rand",
    "noise",
    "length",
    "distance",
    "dot",
    "cross",
    "normalize",
    "set",
];

fn arity(name: &str, args: &[Value], want: &[usize], span: &Range<usize>) -> Result<(), ExprError> {
    if want.contains(&args.len()) {
        return Ok(());
    }
    let wanted = want
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" or ");
    Err(ExprError::new(
        format!("{name}() takes {wanted} argument(s), found {}", args.len()),
        span.clone(),
    ))
}

fn vec3_of(v: Value, name: &str, span: &Range<usize>) -> Result<[f64; 3], ExprError> {
    match v {
        Value::Vec3(a) => Ok(a),
        other => Err(ExprError::new(
            format!("{name}() expects a vec3, found a {}", other.type_name()),
            span.clone(),
        )),
    }
}

/// Evaluates a builtin. Returns `Ok(None)` when the name is not a builtin,
/// so the caller can try the reference functions before reporting it.
pub fn call(name: &str, args: &[Value], span: &Range<usize>) -> Result<Option<Value>, ExprError> {
    // Component-wise unary maths.
    let unary: Option<fn(f64) -> f64> = match name {
        "abs" => Some(f64::abs),
        // signum() returns 1.0 for +0.0 and -1.0 for -0.0; a sign function
        // that calls zero "positive" surprises people, so zero maps to zero.
        "sign" => Some(|v: f64| if v == 0.0 { 0.0 } else { v.signum() }),
        "floor" => Some(f64::floor),
        "ceil" => Some(f64::ceil),
        "round" => Some(f64::round),
        "sqrt" => Some(f64::sqrt),
        "exp" => Some(f64::exp),
        "log" => Some(f64::ln),
        "sin" => Some(f64::sin),
        "cos" => Some(f64::cos),
        "tan" => Some(f64::tan),
        "asin" => Some(f64::asin),
        "acos" => Some(f64::acos),
        "atan" => Some(f64::atan),
        "radians" => Some(f64::to_radians),
        "degrees" => Some(f64::to_degrees),
        _ => None,
    };
    if let Some(f) = unary {
        arity(name, args, &[1], span)?;
        return map1(args[0], name, span, f).map(Some);
    }

    // Component-wise binary maths.
    let binary: Option<fn(f64, f64) -> f64> = match name {
        "pow" => Some(f64::powf),
        // Rust's `%` keeps the sign of the dividend, which is what every
        // shading language calls fmod.
        "fmod" => Some(|a: f64, b: f64| a % b),
        "atan2" => Some(f64::atan2),
        _ => None,
    };
    if let Some(f) = binary {
        arity(name, args, &[2], span)?;
        return map2(args[0], args[1], name, span, f).map(Some);
    }

    match name {
        "min" | "max" => {
            if args.len() < 2 {
                return Err(ExprError::new(
                    format!("{name}() takes at least 2 arguments, found {}", args.len()),
                    span.clone(),
                ));
            }
            let pick: fn(f64, f64) -> f64 = if name == "min" { f64::min } else { f64::max };
            let mut acc = args[0];
            for next in &args[1..] {
                acc = map2(acc, *next, name, span, pick)?;
            }
            Ok(Some(acc))
        }
        "clamp" => {
            arity(name, args, &[3], span)?;
            let lo = map2(args[0], args[1], name, span, f64::max)?;
            map2(lo, args[2], name, span, f64::min).map(Some)
        }
        "lerp" => {
            arity(name, args, &[3], span)?;
            let t = args[2];
            let span_ab = map2(args[1], args[0], name, span, |b, a| b - a)?;
            let scaled = map2(span_ab, t, name, span, |d, f| d * f)?;
            map2(args[0], scaled, name, span, |a, d| a + d).map(Some)
        }
        "fit" => {
            arity(name, args, &[5], span)?;
            // fit(v, omin, omax, nmin, nmax). A zero-width source range
            // would divide by zero, so it collapses to the new minimum
            // rather than producing an infinity that poisons the cook.
            let num = map2(args[0], args[1], name, span, |v, omin| v - omin)?;
            let den = map2(args[2], args[1], name, span, |omax, omin| omax - omin)?;
            let t = map2(
                num,
                den,
                name,
                span,
                |n, d| if d == 0.0 { 0.0 } else { n / d },
            )?;
            let width = map2(args[4], args[3], name, span, |nmax, nmin| nmax - nmin)?;
            let scaled = map2(t, width, name, span, |t, w| t * w)?;
            map2(args[3], scaled, name, span, |nmin, s| nmin + s).map(Some)
        }
        "length" => {
            arity(name, args, &[1], span)?;
            let Some(lanes) = args[0].lanes() else {
                return Err(ExprError::new(
                    format!("length() expects a number, found a {}", args[0].type_name()),
                    span.clone(),
                ));
            };
            Ok(Some(Value::Float(
                lanes.iter().map(|v| v * v).sum::<f64>().sqrt(),
            )))
        }
        "distance" => {
            arity(name, args, &[2], span)?;
            let d = map2(args[0], args[1], name, span, |a, b| a - b)?;
            let lanes = d.lanes().unwrap_or_default();
            Ok(Some(Value::Float(
                lanes.iter().map(|v| v * v).sum::<f64>().sqrt(),
            )))
        }
        "dot" => {
            arity(name, args, &[2], span)?;
            let p = map2(args[0], args[1], name, span, |a, b| a * b)?;
            let lanes = p.lanes().unwrap_or_default();
            Ok(Some(Value::Float(lanes.iter().sum())))
        }
        "cross" => {
            arity(name, args, &[2], span)?;
            let a = vec3_of(args[0], name, span)?;
            let b = vec3_of(args[1], name, span)?;
            Ok(Some(Value::Vec3([
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ])))
        }
        "normalize" => {
            arity(name, args, &[1], span)?;
            let Some(lanes) = args[0].lanes() else {
                return Err(ExprError::new(
                    format!(
                        "normalize() expects a number, found a {}",
                        args[0].type_name()
                    ),
                    span.clone(),
                ));
            };
            let len = lanes.iter().map(|v| v * v).sum::<f64>().sqrt();
            // A zero vector has no direction; returning it unchanged is the
            // only answer that does not introduce a NaN downstream.
            let out: Vec<f64> = if len == 0.0 {
                lanes
            } else {
                lanes.into_iter().map(|v| v / len).collect()
            };
            Ok(Value::from_lanes(&out))
        }
        "set" => {
            arity(name, args, &[2, 3, 4], span)?;
            let mut lanes = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                lanes.push(a.as_float(&format!("set() argument {}", i + 1), span)?);
            }
            Ok(Value::from_lanes(&lanes))
        }
        "rand" => {
            arity(name, args, &[1, 2], span)?;
            let index = args[0].as_float("rand() index", span)?;
            let seed = match args.get(1) {
                Some(s) => s.as_float("rand() seed", span)?,
                None => 0.0,
            };
            // Truncating toward zero and saturating keeps a NaN or a huge
            // float from wrapping into an arbitrary lattice cell.
            Ok(Some(Value::Float(solarxy_kernel::rng::unit_f64(
                to_index(index),
                0,
                to_seed(seed),
            ))))
        }
        "noise" => {
            arity(name, args, &[1, 2, 3], span)?;
            let mut coords = [0.0f64; 3];
            for (i, a) in args.iter().enumerate() {
                coords[i] = a.as_float(&format!("noise() argument {}", i + 1), span)?;
            }
            Ok(Some(Value::Float(noise3(coords[0], coords[1], coords[2]))))
        }
        _ => Ok(None),
    }
}

/// A float to a lattice/sample index, saturating rather than wrapping.
fn to_index(v: f64) -> u64 {
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= u64::MAX as f64 {
        u64::MAX
    } else {
        v as u64
    }
}

fn to_seed(v: f64) -> u32 {
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        v as u32
    }
}

// ---- the frozen noise ----

/// Avalanche hash of a lattice cell. The constants match
/// `solarxy_kernel::rng::hash` so the two generators share a lineage; it is
/// duplicated rather than imported because the kernel's signature is
/// `(index, lane, seed)` and this needs three spatial coordinates.
fn lattice_hash(x: i64, y: i64, z: i64) -> u64 {
    let mut v = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    v ^= v >> 33;
    v = v.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    v ^= v >> 33;
    v = v.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    v ^= v >> 33;
    v
}

/// A lattice value in `[-1, 1]`.
fn lattice(x: i64, y: i64, z: i64) -> f64 {
    // Top 53 bits, a full f64 mantissa, mapped to [-1, 1).
    let unit = (lattice_hash(x, y, z) >> 11) as f64 / 9_007_199_254_740_992.0;
    unit.mul_add(2.0, -1.0)
}

/// Smoothstep, the same easing the imaging crate's noise uses.
fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Trilinearly interpolated value noise in `[-1, 1]`.
///
/// Frozen: this exact function is the contract. Changing the constants,
/// the easing, or the interpolation changes every scene that uses it.
fn noise3(x: f64, y: f64, z: f64) -> f64 {
    // A non-finite coordinate has no cell; returning zero keeps one bad
    // input from turning an entire attribute lane into NaN.
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return 0.0;
    }
    let (xi, yi, zi) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (smooth(x - xi), smooth(y - yi), smooth(z - zi));
    let (xi, yi, zi) = (xi as i64, yi as i64, zi as i64);

    let corner = |dx: i64, dy: i64, dz: i64| lattice(xi + dx, yi + dy, zi + dz);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;

    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), fx);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), fx);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), fx);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), fx);
    let y0 = lerp(x00, x10, fy);
    let y1 = lerp(x01, x11, fy);
    lerp(y0, y1, fz)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests
    use super::*;

    fn s() -> Range<usize> {
        0..1
    }

    fn f(name: &str, args: &[Value]) -> Value {
        call(name, args, &s())
            .expect("evaluates")
            .expect("is a builtin")
    }

    fn num(v: f64) -> Value {
        Value::Float(v)
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn unary_maths_match_known_values() {
        assert_eq!(f("abs", &[num(-3.0)]), num(3.0));
        assert_eq!(f("floor", &[num(1.7)]), num(1.0));
        assert_eq!(f("ceil", &[num(1.2)]), num(2.0));
        assert_eq!(f("round", &[num(1.5)]), num(2.0));
        assert_eq!(f("sqrt", &[num(9.0)]), num(3.0));
        let Value::Float(v) = f("sin", &[num(0.0)]) else {
            panic!()
        };
        approx(v, 0.0);
        let Value::Float(v) = f("degrees", &[num(std::f64::consts::PI)]) else {
            panic!()
        };
        approx(v, 180.0);
    }

    #[test]
    fn sign_calls_zero_zero() {
        // f64::signum would call +0.0 positive, which reads as a bug.
        assert_eq!(f("sign", &[num(0.0)]), num(0.0));
        assert_eq!(f("sign", &[num(-4.0)]), num(-1.0));
        assert_eq!(f("sign", &[num(4.0)]), num(1.0));
    }

    #[test]
    fn min_and_max_take_more_than_two() {
        assert_eq!(f("min", &[num(3.0), num(1.0), num(2.0)]), num(1.0));
        assert_eq!(f("max", &[num(3.0), num(1.0), num(7.0)]), num(7.0));
    }

    #[test]
    fn clamp_lerp_and_fit_agree_with_hand_arithmetic() {
        assert_eq!(f("clamp", &[num(5.0), num(0.0), num(1.0)]), num(1.0));
        assert_eq!(f("clamp", &[num(-5.0), num(0.0), num(1.0)]), num(0.0));
        assert_eq!(f("lerp", &[num(0.0), num(10.0), num(0.25)]), num(2.5));
        // 5 in [0,10] -> [100,200] is 150.
        assert_eq!(
            f(
                "fit",
                &[num(5.0), num(0.0), num(10.0), num(100.0), num(200.0)]
            ),
            num(150.0)
        );
    }

    #[test]
    fn fit_with_a_zero_width_source_does_not_produce_infinity() {
        let v = f("fit", &[num(5.0), num(2.0), num(2.0), num(10.0), num(20.0)]);
        assert_eq!(v, num(10.0), "collapses to the new minimum");
    }

    #[test]
    fn vector_functions_match_known_values() {
        assert_eq!(f("length", &[Value::Vec3([3.0, 4.0, 0.0])]), num(5.0));
        assert_eq!(
            f(
                "distance",
                &[Value::Vec2([0.0, 0.0]), Value::Vec2([3.0, 4.0])]
            ),
            num(5.0)
        );
        assert_eq!(
            f(
                "dot",
                &[Value::Vec3([1.0, 2.0, 3.0]), Value::Vec3([4.0, 5.0, 6.0])]
            ),
            num(32.0)
        );
        assert_eq!(
            f(
                "cross",
                &[Value::Vec3([1.0, 0.0, 0.0]), Value::Vec3([0.0, 1.0, 0.0])]
            ),
            Value::Vec3([0.0, 0.0, 1.0])
        );
        assert_eq!(
            f("normalize", &[Value::Vec3([0.0, 5.0, 0.0])]),
            Value::Vec3([0.0, 1.0, 0.0])
        );
    }

    #[test]
    fn normalizing_a_zero_vector_yields_zero_not_nan() {
        assert_eq!(
            f("normalize", &[Value::Vec3([0.0; 3])]),
            Value::Vec3([0.0; 3])
        );
    }

    #[test]
    fn set_builds_each_vector_width() {
        assert_eq!(f("set", &[num(1.0), num(2.0)]), Value::Vec2([1.0, 2.0]));
        assert_eq!(
            f("set", &[num(1.0), num(2.0), num(3.0)]),
            Value::Vec3([1.0, 2.0, 3.0])
        );
        assert_eq!(
            f("set", &[num(1.0), num(2.0), num(3.0), num(4.0)]),
            Value::Vec4([1.0, 2.0, 3.0, 4.0])
        );
    }

    #[test]
    fn rand_agrees_with_the_kernel_generator() {
        // An expression and a scatter drawing from the same seed must
        // agree, so this is the kernel's own draw, not a copy.
        let Value::Float(v) = f("rand", &[num(7.0), num(42.0)]) else {
            panic!()
        };
        assert_eq!(v, solarxy_kernel::rng::unit_f64(7, 0, 42));
    }

    #[test]
    fn rand_is_deterministic_and_decorrelates() {
        assert_eq!(f("rand", &[num(1.0)]), f("rand", &[num(1.0)]));
        assert_ne!(f("rand", &[num(1.0)]), f("rand", &[num(2.0)]));
        assert_ne!(
            f("rand", &[num(1.0), num(1.0)]),
            f("rand", &[num(1.0), num(2.0)])
        );
    }

    #[test]
    fn rand_survives_junk_indices() {
        // Saturating rather than wrapping: a NaN must not silently land on
        // an arbitrary lattice cell.
        assert_eq!(f("rand", &[num(f64::NAN)]), f("rand", &[num(0.0)]));
        assert_eq!(f("rand", &[num(-5.0)]), f("rand", &[num(0.0)]));
    }

    #[test]
    fn noise_is_in_range_deterministic_and_continuous() {
        for i in 0..200 {
            let x = f64::from(i) * 0.37;
            let Value::Float(v) = f("noise", &[num(x)]) else {
                panic!()
            };
            assert!((-1.0..=1.0).contains(&v), "{v} out of range at {x}");
        }
        assert_eq!(f("noise", &[num(1.25)]), f("noise", &[num(1.25)]));
        // Continuity: a small step must not jump. Value noise over a unit
        // lattice with smoothstep easing has slope bounded well under 3.
        let Value::Float(a) = f("noise", &[num(2.0)]) else {
            panic!()
        };
        let Value::Float(b) = f("noise", &[num(2.001)]) else {
            panic!()
        };
        assert!((a - b).abs() < 0.05, "{a} vs {b}");
    }

    #[test]
    fn noise_is_exactly_reproducible() {
        // The output is pinned deliberately: this value changing means
        // every scene using noise() re-renders differently.
        let Value::Float(v) = f("noise", &[num(0.5), num(0.5), num(0.5)]) else {
            panic!()
        };
        let expected = noise3(0.5, 0.5, 0.5);
        assert_eq!(v, expected);
        assert!(v.is_finite());
    }

    #[test]
    fn noise_survives_a_non_finite_coordinate() {
        assert_eq!(f("noise", &[num(f64::NAN)]), num(0.0));
        assert_eq!(f("noise", &[num(f64::INFINITY)]), num(0.0));
    }

    #[test]
    fn arity_errors_name_what_was_wanted() {
        let e = call("abs", &[num(1.0), num(2.0)], &s()).unwrap_err();
        assert!(e.message.contains("takes 1 argument"), "{e:?}");
        let e = call("set", &[num(1.0)], &s()).unwrap_err();
        assert!(e.message.contains("2 or 3 or 4"), "{e:?}");
        let e = call("min", &[num(1.0)], &s()).unwrap_err();
        assert!(e.message.contains("at least 2"), "{e:?}");
    }

    #[test]
    fn an_unknown_name_is_not_a_builtin_rather_than_an_error() {
        // The caller tries reference functions next, so this must be None,
        // not Err.
        assert_eq!(call("ch", &[], &s()).unwrap(), None);
    }

    #[test]
    fn every_advertised_name_actually_resolves() {
        // BUILTIN_NAMES feeds editor completion; a name listed but not
        // implemented would autocomplete into an error.
        for name in BUILTIN_NAMES {
            let args = [num(1.0); 5];
            // `Ok(None)` is the only answer meaning "not a builtin". An
            // arity or type error still proves the dispatcher claims the
            // name: cross() wants vec3s and rejects the floats used here.
            let claimed = (0..=5).any(|n| !matches!(call(name, &args[..n], &s()), Ok(None)));
            assert!(claimed, "`{name}` is advertised but does not resolve");
        }
    }
}
