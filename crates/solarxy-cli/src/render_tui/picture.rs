//! Turning a render's pixels into something a terminal can hold.
//!
//! # Why the picture is grey
//!
//! Not a limitation, a rule. This surface may use colour for accent, success,
//! warning and error and for nothing else, and body text is terminal ink. A
//! colour picture would break exactly the assertions the dashboard is required
//! to pass, and it would break them for a decoration. What survives the rule is
//! shape, and shape is what a reader is watching for: whether the framing is
//! right, whether the light landed, whether the render is worth waiting for.
//!
//! # One sample a cell
//!
//! The half-block trick doubles vertical resolution by painting one pixel as
//! the foreground and the one below it as the **background**, and this surface
//! never paints a background. So a cell is one sample, and the aspect is
//! corrected by sampling: terminal cells are about twice as tall as they are
//! wide, so a cell covers twice as much of the picture vertically as it does
//! horizontally.

use super::state::Picture;
use crate::tui::caps::Glyphs;

/// How much taller a terminal cell is than it is wide.
///
/// Not measured, because it cannot be: the terminal knows and does not say.
/// Two is the ratio every monospace font used for this is near, and being a
/// little wrong here squashes a picture slightly rather than breaking it.
const CELL_ASPECT: u32 = 2;

impl Picture {
    /// Reduce an eight-bit colour image to luminance.
    ///
    /// Rec. 709 weights, because the picture is display-referred by the time
    /// it reaches here and those are the weights that describe what a person
    /// sees rather than what the channels hold.
    pub fn from_rgba8(width: u32, height: u32, pixels: &[u8]) -> Self {
        let luma = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                let value =
                    0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2]);
                value.round().clamp(0.0, 255.0) as u8
            })
            .collect();
        Self {
            width,
            height,
            luma,
        }
    }

    /// Reduce a four-float image the same way, with a clamp.
    ///
    /// The values may be scene-referred, in which case anything above one is
    /// light this preview cannot show and the clamp is the whole tone map.
    /// Said plainly in the reference rather than pretended otherwise: the
    /// window and the file are where a float render is judged.
    pub fn from_rgba32f(width: u32, height: u32, pixels: &[u8]) -> Self {
        let luma = pixels
            .as_chunks::<16>()
            .0
            .iter()
            .map(|p| {
                let channel = |i: usize| {
                    f32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]).clamp(0.0, 1.0)
                };
                let value = 0.2126 * channel(0) + 0.7152 * channel(4) + 0.0722 * channel(8);
                (value * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect();
        Self {
            width,
            height,
            luma,
        }
    }

    /// The largest cell rectangle inside `cells` that keeps the picture's own
    /// proportions, given how tall a cell is.
    ///
    /// Returned rather than drawn into the whole area, because a picture
    /// stretched to fill a panel is a picture that lies about the framing,
    /// which is the one thing a reader is looking at it for.
    pub fn fit(&self, columns: u16, rows: u16) -> (u16, u16) {
        if self.width == 0 || self.height == 0 || columns == 0 || rows == 0 {
            return (0, 0);
        }
        // Cell columns per cell row, if the picture filled the width.
        let wanted_rows = (u32::from(columns) * self.height) / (self.width * CELL_ASPECT);
        if wanted_rows <= u32::from(rows) {
            (columns, wanted_rows.max(1) as u16)
        } else {
            let wanted_columns = (u32::from(rows) * self.width * CELL_ASPECT) / self.height;
            (wanted_columns.max(1).min(u32::from(columns)) as u16, rows)
        }
    }

    /// The narrowest span of luminance the picture actually uses.
    ///
    /// Five rungs over the whole of zero to one is coarse enough that an
    /// ordinary render lands almost entirely on two of them: measured on the
    /// sample orrery, the background and the subject both fell on the same
    /// rung and the picture read as a flat field. Stretched to its own range
    /// the same picture separates.
    ///
    /// A span this narrow is not stretched, because a genuinely flat picture
    /// stretched to the full ramp is noise amplified to look like an image.
    const FLAT: u8 = 8;

    fn range(&self) -> (u8, u8) {
        let lo = self.luma.iter().copied().min().unwrap_or(0);
        let hi = self.luma.iter().copied().max().unwrap_or(0);
        (lo, hi)
    }

    /// The picture as rows of shading, one string a row.
    ///
    /// Nearest sampling. A box filter would be better and this is a preview of
    /// a render that is still moving, so the cheaper one is the honest choice:
    /// what it costs is a little aliasing on a picture that is already five
    /// levels deep.
    ///
    /// **Contrast is stretched to the picture's own range**, which makes this a
    /// picture of the shape rather than of the exposure. That is the trade the
    /// five rungs force, and it is the right way round: whether the framing and
    /// the lighting landed is what a reader is watching for, and whether the
    /// exposure is right is a question for the file.
    pub fn rows(&self, columns: u16, rows: u16, glyphs: &Glyphs) -> Vec<String> {
        if columns == 0 || rows == 0 || self.luma.is_empty() {
            return Vec::new();
        }
        let (lo, hi) = self.range();
        let stretch = hi.saturating_sub(lo) >= Self::FLAT;
        let span = f64::from(hi.saturating_sub(lo)).max(1.0);
        (0..u32::from(rows))
            .map(|row| {
                let y = (row * self.height / u32::from(rows)).min(self.height - 1);
                (0..u32::from(columns))
                    .map(|column| {
                        let x = (column * self.width / u32::from(columns)).min(self.width - 1);
                        let at = (y * self.width + x) as usize;
                        let value = self.luma.get(at).copied().unwrap_or(0);
                        let fraction = if stretch {
                            f64::from(value.saturating_sub(lo)) / span
                        } else {
                            f64::from(value) / 255.0
                        };
                        glyphs.shade(fraction)
                    })
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::caps::GlyphTier;

    fn flat(width: u32, height: u32, value: u8) -> Picture {
        Picture {
            width,
            height,
            luma: vec![value; (width * height) as usize],
        }
    }

    /// The reduction is a luminance, not an average: green carries most of it
    /// and blue almost none, which is what makes a preview of a blue sky read
    /// as dark rather than as mid grey.
    #[test]
    fn the_reduction_weights_the_channels_the_way_an_eye_does() {
        let pixels = [
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
        ];
        let picture = Picture::from_rgba8(3, 1, &pixels);
        assert_eq!(picture.luma, vec![54, 182, 18]);
    }

    /// A picture is never stretched to fill the panel. Cells are twice as tall
    /// as they are wide, so a square picture in a square panel uses half the
    /// rows.
    #[test]
    fn the_fit_keeps_the_pictures_proportions() {
        let square = flat(100, 100, 0);
        assert_eq!(square.fit(40, 40), (40, 20));
        // And where the height is the binding constraint, the width gives way.
        assert_eq!(square.fit(40, 10), (20, 10));
    }

    /// A wide picture is bounded by the panel's width rather than overrunning
    /// it, which is the case a fit computed only one way gets wrong.
    #[test]
    fn a_picture_wider_than_the_panel_is_bounded_by_it() {
        let wide = flat(400, 100, 0);
        let (columns, rows) = wide.fit(40, 40);
        assert!(columns <= 40 && rows <= 40, "{columns}x{rows}");
        assert_eq!((columns, rows), (40, 5));
    }

    /// Black is the empty rung and white is the full one, at both tiers, or
    /// the picture is drawn in a language the reader's terminal does not have.
    #[test]
    fn both_tiers_span_their_own_ramp() {
        for tier in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let glyphs = Glyphs::for_tier(tier);
            let dark = flat(4, 2, 0).rows(4, 2, &glyphs);
            let light = flat(4, 2, 255).rows(4, 2, &glyphs);
            assert_eq!(dark, vec!["    ", "    "], "{tier:?}");
            let full = glyphs.density()[4].repeat(4);
            assert_eq!(light, vec![full.clone(), full], "{tier:?}");
        }
    }

    /// A picture using a narrow band of the range still separates, which is
    /// the case that sent this to a stretch: an ordinary render put its
    /// background and its subject on the same rung and read as a flat field.
    #[test]
    fn a_low_contrast_picture_is_stretched_to_its_own_range() {
        let glyphs = Glyphs::for_tier(GlyphTier::Unicode);
        // A background and a subject twenty levels apart, both near the top.
        let picture = Picture {
            width: 4,
            height: 1,
            luma: vec![200, 200, 220, 200],
        };
        let row = &picture.rows(4, 1, &glyphs)[0];
        let marks: Vec<char> = row.chars().collect();
        assert_ne!(marks[0], marks[2], "the subject did not separate: {row:?}");
        assert_eq!(marks[0], ' ', "the background did not fall to the floor");
        assert_eq!(
            marks[2],
            glyphs.density()[4].chars().next().unwrap(),
            "the subject did not reach the top"
        );
    }

    /// And a picture with nothing in it stays flat rather than being stretched
    /// into a field of noise that looks like an image.
    #[test]
    fn a_flat_picture_is_not_stretched_into_one() {
        let glyphs = Glyphs::for_tier(GlyphTier::Unicode);
        let picture = Picture {
            width: 4,
            height: 1,
            luma: vec![128, 129, 130, 131],
        };
        let row = &picture.rows(4, 1, &glyphs)[0];
        let marks: std::collections::HashSet<char> = row.chars().collect();
        assert_eq!(marks.len(), 1, "four near-identical values drew {row:?}");
    }

    /// Sampling reads the picture rather than one corner of it: a gradient
    /// drawn small still runs from one end of the ramp to the other.
    #[test]
    fn a_gradient_survives_being_drawn_small() {
        let width = 64;
        let picture = Picture {
            width,
            height: 1,
            luma: (0..width).map(|x| (x * 255 / (width - 1)) as u8).collect(),
        };
        let glyphs = Glyphs::for_tier(GlyphTier::Unicode);
        let rows = picture.rows(8, 1, &glyphs);
        let row = &rows[0];
        assert!(row.starts_with(' '), "{row:?}");
        assert!(row.ends_with(glyphs.density()[4]), "{row:?}");
    }
}
