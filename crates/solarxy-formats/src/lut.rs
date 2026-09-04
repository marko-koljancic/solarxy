//! Colour lookup table decode: Adobe `.cube` into [`LutCube`].
//!
//! A grading LUT arrives from the same untrusted places a model does, and
//! more often hand-edited, so nothing here trusts a declared size, a token
//! count, or a float, and every diagnostic carries the 1-based line number
//! the reader would see in an editor.
//!
//! Only the 3D form is read. A `.cube` may also carry a 1D table, which is
//! a different thing entirely (a per-channel curve, not a colour cube), so
//! it is refused by name rather than misread as a short 3D table.

use solarxy_core::{LUT_MAX_SIZE, LUT_MIN_SIZE, LutCube};

use crate::FormatsError;

/// The `.cube` default input domain, per the specification: the table
/// covers 0 to 1 unless the file says otherwise.
const DEFAULT_DOMAIN_MIN: [f32; 3] = [0.0; 3];
const DEFAULT_DOMAIN_MAX: [f32; 3] = [1.0; 3];

fn err(line: usize, message: impl Into<String>) -> FormatsError {
    FormatsError::Lut {
        line,
        message: message.into(),
    }
}

/// Parse three whitespace-separated finite floats, the shape both the
/// domain keywords and every table row take.
fn triple(tokens: &[&str], line: usize, what: &str) -> Result<[f32; 3], FormatsError> {
    if tokens.len() != 3 {
        return Err(err(
            line,
            format!(
                "{what} needs exactly 3 numbers, found {}: {:?}",
                tokens.len(),
                tokens
            ),
        ));
    }
    let mut out = [0.0f32; 3];
    for (slot, token) in out.iter_mut().zip(tokens) {
        let v: f32 = token
            .parse()
            .map_err(|_| err(line, format!("{what} component '{token}' is not a number")))?;
        if !v.is_finite() {
            return Err(err(
                line,
                format!("{what} component '{token}' is not finite"),
            ));
        }
        *slot = v;
    }
    Ok(out)
}

/// Decode an Adobe `.cube` colour lookup table.
///
/// Accepts the 3D form with an optional `TITLE` and optional `DOMAIN_MIN` /
/// `DOMAIN_MAX`, followed by exactly `LUT_3D_SIZE` cubed rows of three
/// floats with **red varying fastest**, which is both the file's ordering
/// and [`LutCube`]'s. Comments (`#`) and blank lines may appear anywhere.
///
/// Table values are deliberately not clamped: a LUT may legitimately map
/// into or out of a range wider than 0 to 1, and the sampler decides what
/// to do about that, not the parser.
pub fn decode_cube_bytes(bytes: &[u8]) -> Result<LutCube, FormatsError> {
    // Lossy rather than strict: the only field that can carry non-ASCII is
    // TITLE, which nothing reads, and failing a whole table over a stray
    // byte in a comment would be the wrong trade for a hand-edited format.
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let mut size: Option<u32> = None;
    let mut domain_min = DEFAULT_DOMAIN_MIN;
    let mut domain_max = DEFAULT_DOMAIN_MAX;
    let mut data: Vec<f32> = Vec::new();
    let mut expected: usize = 0;

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        // A comment may trail content, and the specification's own samples
        // put one after the table size.
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = content.split_whitespace().collect();
        let keyword = tokens[0];
        let rest = &tokens[1..];

        if keyword.eq_ignore_ascii_case("TITLE") {
            continue;
        }
        if keyword.eq_ignore_ascii_case("LUT_1D_SIZE") {
            return Err(err(
                line,
                "this is a 1D lookup table; Solarxy reads 3D .cube tables \
                 (LUT_3D_SIZE) only. Re-export it as a 3D LUT",
            ));
        }
        if keyword.eq_ignore_ascii_case("LUT_3D_SIZE") {
            if size.is_some() {
                return Err(err(line, "LUT_3D_SIZE declared more than once"));
            }
            let [n] = rest else {
                return Err(err(
                    line,
                    format!("LUT_3D_SIZE needs exactly one number, found {}", rest.len()),
                ));
            };
            let n: u32 = n
                .parse()
                .map_err(|_| err(line, format!("LUT_3D_SIZE '{n}' is not a whole number")))?;
            if !(LUT_MIN_SIZE..=LUT_MAX_SIZE).contains(&n) {
                return Err(err(
                    line,
                    format!(
                        "LUT_3D_SIZE {n} is outside the supported range \
                         {LUT_MIN_SIZE} to {LUT_MAX_SIZE}"
                    ),
                ));
            }
            // Cubed, so the cap above is what keeps this allocation sane.
            expected = (n as usize).pow(3) * 3;
            data.reserve_exact(expected);
            size = Some(n);
            continue;
        }
        if keyword.eq_ignore_ascii_case("DOMAIN_MIN") {
            domain_min = triple(rest, line, "DOMAIN_MIN")?;
            continue;
        }
        if keyword.eq_ignore_ascii_case("DOMAIN_MAX") {
            domain_max = triple(rest, line, "DOMAIN_MAX")?;
            continue;
        }

        // Anything else must be a table row, which means the size has to
        // have been declared already: without it there is no way to know
        // how many rows to expect, and a table read into an unknown shape
        // is worse than a refusal.
        if size.is_none() {
            return Err(err(
                line,
                format!("table data before LUT_3D_SIZE (first token '{keyword}')"),
            ));
        }
        if data.len() >= expected {
            return Err(err(
                line,
                format!(
                    "more table rows than LUT_3D_SIZE declares ({} expected)",
                    expected / 3
                ),
            ));
        }
        data.extend_from_slice(&triple(&tokens, line, "table row")?);
    }

    let Some(size) = size else {
        return Err(err(
            text.lines().count().max(1),
            "no LUT_3D_SIZE: this does not look like a 3D .cube table",
        ));
    };
    if data.len() != expected {
        return Err(err(
            text.lines().count().max(1),
            format!(
                "table is short: {} rows of the {} LUT_3D_SIZE {size} declares",
                data.len() / 3,
                expected / 3
            ),
        ));
    }
    for (channel, (lo, hi)) in domain_min.iter().zip(domain_max.iter()).enumerate() {
        if hi <= lo {
            return Err(err(
                text.lines().count().max(1),
                format!(
                    "DOMAIN_MAX must exceed DOMAIN_MIN on every channel; \
                     channel {channel} has {lo} to {hi}"
                ),
            ));
        }
    }

    Ok(LutCube::new(size, data, domain_min, domain_max))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact literals parsed from the table text

    use super::*;

    /// The smallest well-formed table: two entries per axis, identity.
    fn identity_2() -> String {
        use std::fmt::Write as _;
        let mut s = String::from("TITLE \"tiny\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    let _ = writeln!(s, "{r}.0 {g}.0 {b}.0");
                }
            }
        }
        s
    }

    #[test]
    fn a_minimal_table_decodes_with_the_default_domain() {
        let lut = decode_cube_bytes(identity_2().as_bytes()).expect("decode");
        assert_eq!(lut.size, 2);
        assert_eq!(lut.data.len(), 8 * 3);
        assert_eq!(lut.domain_min, [0.0; 3]);
        assert_eq!(lut.domain_max, [1.0; 3]);
        assert_eq!(lut, LutCube::identity(2));
    }

    #[test]
    fn red_varies_fastest() {
        let lut = decode_cube_bytes(identity_2().as_bytes()).expect("decode");
        // Entry 1 is (r=1, g=0, b=0): the second row of the file.
        assert_eq!(&lut.data[3..6], &[1.0, 0.0, 0.0]);
        // Entry 2 is (r=0, g=1, b=0), so green moved only after red wrapped.
        assert_eq!(&lut.data[6..9], &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn comments_blank_lines_and_crlf_are_ignored() {
        let mut src = "# a comment\r\n\r\nLUT_3D_SIZE 2 # trailing comment\r\n".to_string();
        for l in identity_2().lines().skip(2) {
            src.push_str(l);
            src.push_str("\r\n");
        }
        let lut = decode_cube_bytes(src.as_bytes()).expect("decode");
        assert_eq!(lut, LutCube::identity(2));
    }

    #[test]
    fn a_declared_domain_is_carried() {
        let src = identity_2().replace(
            "LUT_3D_SIZE 2",
            "DOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 4.0 4.0 4.0\nLUT_3D_SIZE 2",
        );
        let lut = decode_cube_bytes(src.as_bytes()).expect("decode");
        assert_eq!(lut.domain_min, [0.0; 3]);
        assert_eq!(lut.domain_max, [4.0; 3]);
        // The domain is part of identity: two tables with the same entries
        // and different domains are different transforms.
        assert_ne!(lut.hash, LutCube::identity(2).hash);
    }

    #[test]
    fn table_values_outside_zero_to_one_are_kept() {
        let src = identity_2().replace("1.0 1.0 1.0", "-0.25 1.75 1.0");
        let lut = decode_cube_bytes(src.as_bytes()).expect("decode");
        assert!(lut.data.contains(&-0.25));
        assert!(lut.data.contains(&1.75));
    }

    /// Every malformed case reports the offending line and none of them
    /// panic. The line numbers are asserted because a diagnostic that
    /// names the wrong line is worse than one that names none.
    #[test]
    fn malformed_tables_are_diagnosed_by_line() {
        let cases: &[(&str, usize, &str)] = &[
            ("LUT_1D_SIZE 32\n0 0 0\n", 1, "1D"),
            ("LUT_3D_SIZE zzz\n", 1, "not a whole number"),
            ("LUT_3D_SIZE 1\n", 1, "outside the supported range"),
            ("LUT_3D_SIZE 4096\n", 1, "outside the supported range"),
            ("0.0 0.0 0.0\n", 1, "before LUT_3D_SIZE"),
            ("LUT_3D_SIZE 2\nLUT_3D_SIZE 2\n", 2, "more than once"),
            ("LUT_3D_SIZE 2\n0.0 0.0\n", 2, "exactly 3 numbers"),
            ("LUT_3D_SIZE 2\n0.0 0.0 wat\n", 2, "is not a number"),
            ("LUT_3D_SIZE 2\nDOMAIN_MIN 0.0 0.0\n", 2, "DOMAIN_MIN needs"),
        ];
        for (src, line, needle) in cases {
            let e = decode_cube_bytes(src.as_bytes()).expect_err(src);
            let FormatsError::Lut {
                line: got,
                ref message,
            } = e
            else {
                panic!("expected a Lut error for {src:?}, got {e:?}");
            };
            assert_eq!(got, *line, "wrong line for {src:?}: {message}");
            assert!(
                message.contains(needle),
                "message for {src:?} missing {needle:?}: {message}"
            );
        }
    }

    #[test]
    fn a_short_table_is_refused_rather_than_padded() {
        let src = "LUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n";
        let e = decode_cube_bytes(src.as_bytes()).expect_err("short");
        assert!(format!("{e}").contains("table is short"), "{e}");
    }

    #[test]
    fn a_long_table_is_refused_rather_than_truncated() {
        let src = identity_2() + "0.5 0.5 0.5\n";
        let e = decode_cube_bytes(src.as_bytes()).expect_err("long");
        assert!(format!("{e}").contains("more table rows"), "{e}");
    }

    #[test]
    fn an_empty_or_headerless_file_names_the_missing_size() {
        for src in ["", "# nothing but a comment\n"] {
            let e = decode_cube_bytes(src.as_bytes()).expect_err(src);
            assert!(format!("{e}").contains("no LUT_3D_SIZE"), "{e}");
        }
    }

    #[test]
    fn an_inverted_domain_is_refused() {
        let src = identity_2().replace(
            "LUT_3D_SIZE 2",
            "LUT_3D_SIZE 2\nDOMAIN_MIN 1.0 0.0 0.0\nDOMAIN_MAX 0.0 1.0 1.0",
        );
        let e = decode_cube_bytes(src.as_bytes()).expect_err("inverted");
        assert!(format!("{e}").contains("DOMAIN_MAX must exceed"), "{e}");
    }

    /// Arbitrary bytes must not panic; the format is text, so garbage is a
    /// diagnostic rather than a crash.
    #[test]
    fn binary_garbage_is_a_diagnostic_not_a_panic() {
        let bytes: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        assert!(decode_cube_bytes(&bytes).is_err());
    }
}
