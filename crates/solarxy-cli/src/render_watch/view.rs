//! Where the picture sits in the window: the letterbox fit, and the view a
//! reader takes by hand with a drag and a wheel.
//!
//! Pure arithmetic over two sizes, which is what makes every rule here
//! testable without a window: the fit recomputes itself from whatever sizes
//! it is asked about, a zoom holds the pixel under the cursor still, and a
//! pan cannot lose the picture off the glass.

/// The image rectangle in window pixels: the top-left corner, then the size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// How far in a wheel can take the picture: window pixels per image pixel.
const ZOOM_CEILING: f32 = 64.0;

/// How far out, as a fraction of the fit scale. Below an eighth of fit the
/// picture is a stamp and the window is canvas; nothing is learned there.
const ZOOM_FLOOR_OF_FIT: f32 = 0.125;

/// How much of the picture a pan must leave on the glass, per axis, in
/// window pixels. Enough to grab and drag back; a picture panned entirely
/// out of view would look exactly like a render that produced nothing.
const MIN_VISIBLE: f32 = 32.0;

/// The view: the fit until the reader takes over, then theirs until reset.
pub struct ViewTransform {
    mode: Mode,
}

enum Mode {
    /// The letterbox, recomputed from the sizes at hand every time it is
    /// asked. That recomputation is what makes a resize refit on its own.
    Fit,
    /// A view taken by hand: window pixels per image pixel, and where the
    /// image's top-left corner sits in the window.
    Free { scale: f32, offset: (f32, f32) },
}

impl ViewTransform {
    pub fn new() -> Self {
        Self { mode: Mode::Fit }
    }

    /// Back to the letterbox fit, which is the default.
    pub fn reset(&mut self) {
        self.mode = Mode::Fit;
    }

    /// Where the picture sits, for these sizes.
    pub fn rect(&self, picture: (u32, u32), window: (u32, u32)) -> Rect {
        match self.mode {
            Mode::Fit => {
                let (x, y, w, h) = letterbox(picture, window);
                Rect { x, y, w, h }
            }
            Mode::Free { scale, offset } => Rect {
                x: offset.0,
                y: offset.1,
                w: picture.0.max(1) as f32 * scale,
                h: picture.1.max(1) as f32 * scale,
            },
        }
    }

    /// Zoom by `factor` keeping the window point `cursor` over the same
    /// image point, which is what makes a wheel feel anchored rather than
    /// drifting toward a corner.
    pub fn zoom_about(
        &mut self,
        cursor: (f32, f32),
        factor: f32,
        picture: (u32, u32),
        window: (u32, u32),
    ) {
        let rect = self.rect(picture, window);
        let scale = rect.w / picture.0.max(1) as f32;
        let fit = fit_scale(picture, window);
        let wanted = (scale * factor).clamp(fit * ZOOM_FLOOR_OF_FIT, ZOOM_CEILING);
        let k = wanted / scale;
        let offset = (
            cursor.0 - (cursor.0 - rect.x) * k,
            cursor.1 - (cursor.1 - rect.y) * k,
        );
        self.mode = Mode::Free {
            scale: wanted,
            offset,
        };
        self.hold_on_glass(picture, window);
    }

    /// Move the picture by a cursor delta in window pixels.
    pub fn pan(&mut self, delta: (f32, f32), picture: (u32, u32), window: (u32, u32)) {
        let rect = self.rect(picture, window);
        self.mode = Mode::Free {
            scale: rect.w / picture.0.max(1) as f32,
            offset: (rect.x + delta.0, rect.y + delta.1),
        };
        self.hold_on_glass(picture, window);
    }

    /// Clamp a free offset so at least [`MIN_VISIBLE`] of the picture stays
    /// in the window on each axis.
    fn hold_on_glass(&mut self, picture: (u32, u32), window: (u32, u32)) {
        let Mode::Free { scale, offset } = &mut self.mode else {
            return;
        };
        let (w, h) = (
            picture.0.max(1) as f32 * *scale,
            picture.1.max(1) as f32 * *scale,
        );
        let (ww, wh) = (window.0.max(1) as f32, window.1.max(1) as f32);
        offset.0 = clamp_lenient(offset.0, MIN_VISIBLE - w, ww - MIN_VISIBLE);
        offset.1 = clamp_lenient(offset.1, MIN_VISIBLE - h, wh - MIN_VISIBLE);
    }
}

/// A clamp that survives an inverted range, which happens when the picture
/// or the window is smaller than the visibility floor.
fn clamp_lenient(v: f32, lo: f32, hi: f32) -> f32 {
    v.clamp(lo.min(hi), hi.max(lo))
}

/// The letterbox scale: window pixels per image pixel when the picture fits.
fn fit_scale(picture: (u32, u32), window: (u32, u32)) -> f32 {
    let (pw, ph) = (picture.0.max(1) as f32, picture.1.max(1) as f32);
    let (ww, wh) = (window.0.max(1) as f32, window.1.max(1) as f32);
    (ww / pw).min(wh / ph)
}

/// The largest rectangle of `into` with the proportions of `picture`, centred.
pub fn letterbox(picture: (u32, u32), into: (u32, u32)) -> (f32, f32, f32, f32) {
    let (pw, ph) = (picture.0.max(1) as f32, picture.1.max(1) as f32);
    let (ww, wh) = (into.0.max(1) as f32, into.1.max(1) as f32);
    let scale = (ww / pw).min(wh / ph);
    let (w, h) = (pw * scale, ph * scale);
    (((ww - w) / 2.0).max(0.0), ((wh - h) / 2.0).max(0.0), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture keeps its proportions inside the window, whichever way round
    /// the two are, and is centred in whatever is left.
    #[test]
    fn the_letterbox_keeps_the_pictures_proportions() {
        // Wider than the window: bars above and below.
        let (x, y, w, h) = letterbox((400, 100), (800, 800));
        assert!(
            (w - 800.0).abs() < 0.01 && (h - 200.0).abs() < 0.01,
            "{w}x{h}"
        );
        assert!(x.abs() < 0.01 && (y - 300.0).abs() < 0.01, "{x},{y}");

        // Taller: bars at the sides.
        let (x, y, w, h) = letterbox((100, 400), (800, 800));
        assert!(
            (w - 200.0).abs() < 0.01 && (h - 800.0).abs() < 0.01,
            "{w}x{h}"
        );
        assert!((x - 300.0).abs() < 0.01 && y.abs() < 0.01, "{x},{y}");

        // Exactly the shape of the window: no bars at all.
        let (x, y, w, h) = letterbox((640, 480), (1280, 960));
        assert!(x.abs() < 0.01 && y.abs() < 0.01, "{x},{y}");
        assert!(
            (w - 1280.0).abs() < 0.01 && (h - 960.0).abs() < 0.01,
            "{w}x{h}"
        );
    }

    /// The default is the fit, a gesture leaves it, and reset returns to it.
    #[test]
    fn reset_returns_to_the_fit() {
        let picture = (400, 300);
        let window = (800, 600);
        let mut view = ViewTransform::new();
        let fit = view.rect(picture, window);

        view.zoom_about((400.0, 300.0), 2.0, picture, window);
        assert!(view.rect(picture, window) != fit, "the zoom did nothing");

        view.reset();
        assert!(
            view.rect(picture, window) == fit,
            "reset did not return to the fit"
        );
    }

    /// The image point under the cursor stays under the cursor through a
    /// zoom, which is the whole of what makes a wheel feel anchored.
    #[test]
    fn a_zoom_holds_the_pixel_under_the_cursor() {
        let picture = (400, 300);
        let window = (800, 600);
        let mut view = ViewTransform::new();
        let cursor = (250.0, 220.0);

        let before = view.rect(picture, window);
        let image_point = (
            (cursor.0 - before.x) / (before.w / 400.0),
            (cursor.1 - before.y) / (before.h / 300.0),
        );

        view.zoom_about(cursor, 2.5, picture, window);

        let after = view.rect(picture, window);
        let held = (
            (cursor.0 - after.x) / (after.w / 400.0),
            (cursor.1 - after.y) / (after.h / 300.0),
        );
        assert!(
            (held.0 - image_point.0).abs() < 0.01 && (held.1 - image_point.1).abs() < 0.01,
            "the anchor drifted: {image_point:?} became {held:?}"
        );
    }

    /// A wheel cannot take the view past its bounds in either direction.
    #[test]
    fn a_zoom_is_clamped_at_both_ends() {
        let picture = (400, 300);
        let window = (800, 600);
        let centre = (400.0, 300.0);

        let mut view = ViewTransform::new();
        for _ in 0..40 {
            view.zoom_about(centre, 4.0, picture, window);
        }
        let rect = view.rect(picture, window);
        assert!(
            (rect.w / 400.0 - ZOOM_CEILING).abs() < 0.01,
            "the ceiling did not hold: {}",
            rect.w / 400.0
        );

        let mut view = ViewTransform::new();
        for _ in 0..40 {
            view.zoom_about(centre, 0.25, picture, window);
        }
        let rect = view.rect(picture, window);
        let floor = fit_scale(picture, window) * ZOOM_FLOOR_OF_FIT;
        assert!(
            (rect.w / 400.0 - floor).abs() < 0.001,
            "the floor did not hold: {}",
            rect.w / 400.0
        );
    }

    /// A pan moves the picture by the drag, and cannot lose it off the glass.
    #[test]
    fn a_pan_moves_and_cannot_lose_the_picture() {
        let picture = (400, 300);
        let window = (800, 600);
        let mut view = ViewTransform::new();

        let before = view.rect(picture, window);
        view.pan((10.0, 20.0), picture, window);
        let after = view.rect(picture, window);
        assert!(
            (after.x - before.x - 10.0).abs() < 0.01 && (after.y - before.y - 20.0).abs() < 0.01,
            "the pan did not move by the drag"
        );

        view.pan((1e6, 1e6), picture, window);
        let gone = view.rect(picture, window);
        assert!(
            gone.x <= 800.0 - MIN_VISIBLE + 0.01 && gone.y <= 600.0 - MIN_VISIBLE + 0.01,
            "the picture left the glass: {gone:?}"
        );

        view.pan((-1e6, -1e6), picture, window);
        let gone = view.rect(picture, window);
        assert!(
            gone.x + gone.w >= MIN_VISIBLE - 0.01 && gone.y + gone.h >= MIN_VISIBLE - 0.01,
            "the picture left the glass the other way: {gone:?}"
        );
    }

    /// The fit tracks the window it is asked about, which is what makes a
    /// resize refit without an event handler doing anything.
    #[test]
    fn the_fit_recomputes_for_a_new_window() {
        let picture = (400, 300);
        let view = ViewTransform::new();
        let small = view.rect(picture, (800, 600));
        let large = view.rect(picture, (1600, 1200));
        assert!(
            (large.w - small.w * 2.0).abs() < 0.01,
            "the fit did not follow the window"
        );
    }
}
