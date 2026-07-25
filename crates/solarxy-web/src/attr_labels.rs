//! Text assembly for the GPU attribute-label channel: formats lane values
//! and point numbers exactly like the retired DOM overlay did (its
//! `fmtPinValue`/`pinText` semantics, JS parity included), then encodes
//! the run into the packed glyph words `label.wgsl` decodes.
//!
//! Deliberately NOT wasm-gated (the `attr_viz` convention): pure string
//! and bit math, so native CI runs the parity tests.

use solarxy_renderer::labels::{LabelInstance, TEXT_MAX, glyph_index, pack_glyph};

/// Two-decimal component formatting, JS-parity: `Math.round(x * 100) / 100`
/// rounds half toward positive infinity (NOT half away from zero, which is
/// what Rust's `round` does and differs on negative halves), minus zero
/// prints as `0`, and the shortest round-trip decimal matches JS `String`.
/// Non-finite values spell themselves out, as the DOM did.
#[must_use]
pub fn fmt_component(component: f32) -> String {
    let wide = f64::from(component);
    if wide.is_nan() {
        return "NaN".to_string();
    }
    if wide.is_infinite() {
        return if wide > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let rounded = (wide * 100.0 + 0.5).floor() / 100.0;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    if rounded.abs() >= 1e21 {
        // The JS exponent form ("1e+21"); Rust's `{:e}` omits the plus.
        let sci = format!("{rounded:e}");
        return match sci.find('e') {
            Some(at) if !sci[at + 1..].starts_with('-') => {
                format!("{}e+{}", &sci[..at], &sci[at + 1..])
            }
            _ => sci,
        };
    }
    format!("{rounded}")
}

/// The joined value text: components at two decimals, comma-space
/// separated.
#[must_use]
pub fn fmt_pin_value(value: &[f32]) -> String {
    value
        .iter()
        .map(|v| fmt_component(*v))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The text one label shows: the value in labels mode (falling back to the
/// point number when the lane is absent or empty), the point number in
/// points mode, both as `"num: value"` when both modes are on.
#[must_use]
pub fn pin_text(ptnum: u64, value: Option<&[f32]>, labels: bool, points: bool) -> String {
    let value_text = match value {
        Some(v) if labels && !v.is_empty() => Some(fmt_pin_value(v)),
        _ => None,
    };
    match value_text {
        Some(v) if points => format!("{ptnum}: {v}"),
        Some(v) => v,
        None => ptnum.to_string(),
    }
}

/// Encodes one label's text into packed glyph words, truncating at the
/// column field's capacity and skipping characters outside the baked
/// charset. Returns the glyph count (== words appended).
pub fn encode(text: &str, label_idx: u32, out: &mut Vec<u32>) -> u32 {
    let mut col = 0u32;
    for c in text.chars() {
        if col > TEXT_MAX {
            break;
        }
        let Some(glyph) = glyph_index(c) else {
            continue;
        };
        out.push(pack_glyph(label_idx, col, glyph));
        col += 1;
    }
    col
}

/// One label candidate from the host's stride walk.
pub struct LabelCandidate {
    pub world: [f32; 3],
    pub ptnum: u64,
    pub value: Option<Vec<f32>>,
}

/// Assembles the full label set for the renderer: one instance per
/// candidate plus the flat glyph stream.
#[must_use]
pub fn build_labels(
    candidates: &[LabelCandidate],
    labels: bool,
    points: bool,
) -> (Vec<LabelInstance>, Vec<u32>) {
    let mut instances = Vec::with_capacity(candidates.len());
    let mut words = Vec::with_capacity(candidates.len() * 8);
    for (i, c) in candidates.iter().enumerate() {
        let text = pin_text(c.ptnum, c.value.as_deref(), labels, points);
        #[allow(clippy::cast_possible_truncation)]
        let glyph_count = encode(&text, i as u32, &mut words);
        instances.push(LabelInstance {
            pos: c.world,
            glyph_count,
        });
    }
    (instances, words)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cases mirrored from the retired attrPins.test.ts, so the GPU
    // channel renders byte-identical text to the DOM overlay it replaced.

    #[test]
    fn rounds_to_two_decimals_and_joins_components() {
        assert_eq!(fmt_pin_value(&[0.125]), "0.13");
        assert_eq!(fmt_pin_value(&[1.0, 0.5, 0.25]), "1, 0.5, 0.25");
    }

    #[test]
    fn normalizes_negative_zero() {
        assert_eq!(fmt_pin_value(&[-0.001]), "0");
    }

    #[test]
    fn negative_halves_round_toward_positive_infinity_like_js() {
        // JS Math.round(-12.5) is -12; Rust's f64::round would say -13.
        assert_eq!(fmt_component(-0.125), "-0.12");
        assert_eq!(fmt_component(0.125), "0.13");
    }

    #[test]
    fn non_finite_values_spell_themselves_out() {
        assert_eq!(fmt_component(f32::NAN), "NaN");
        assert_eq!(fmt_component(f32::INFINITY), "Infinity");
        assert_eq!(fmt_component(f32::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn pin_text_modes() {
        assert_eq!(pin_text(7, Some(&[0.5]), true, false), "0.5");
        assert_eq!(pin_text(7, None, false, true), "7");
        assert_eq!(pin_text(7, Some(&[0.5]), true, true), "7: 0.5");
        assert_eq!(pin_text(7, None, true, false), "7");
        assert_eq!(pin_text(7, Some(&[]), true, true), "7");
    }

    #[test]
    fn encode_packs_columns_and_truncates() {
        let mut words = Vec::new();
        let n = encode("1, 2", 3, &mut words);
        assert_eq!(n, 4);
        assert_eq!(words.len(), 4);
        for (col, w) in words.iter().enumerate() {
            assert_eq!(w >> 11, 3, "label index");
            assert_eq!((w >> 5) & 63, u32::try_from(col).unwrap(), "column");
        }

        let long = "1".repeat(200);
        let mut words = Vec::new();
        let n = encode(&long, 0, &mut words);
        assert_eq!(n, TEXT_MAX + 1, "truncates at the column capacity");
    }

    #[test]
    fn encode_skips_characters_outside_the_charset() {
        let mut words = Vec::new();
        // 'x' is not baked; the run must not leave a hole in the columns.
        let n = encode("1x2", 0, &mut words);
        assert_eq!(n, 2);
        assert_eq!((words[1] >> 5) & 63, 1);
    }

    #[test]
    fn build_labels_counts_match_the_stream() {
        let candidates = vec![
            LabelCandidate {
                world: [0.0, 1.0, 2.0],
                ptnum: 0,
                value: Some(vec![0.5, -0.25]),
            },
            LabelCandidate {
                world: [3.0, 4.0, 5.0],
                ptnum: 1,
                value: None,
            },
        ];
        let (instances, words) = build_labels(&candidates, true, true);
        assert_eq!(instances.len(), 2);
        let total: u32 = instances.iter().map(|i| i.glyph_count).sum();
        assert_eq!(total as usize, words.len());
        // "0: 0.5, -0.25" is 13 glyphs; "1" alone (no lane) is 1.
        assert_eq!(instances[0].glyph_count, 13);
        assert_eq!(instances[1].glyph_count, 1);
    }
}
