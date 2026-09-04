// Where the picture sits in the render window: the letterbox fit, and the view
// a reader takes by hand with a drag and a wheel.
//
// A port of `render_watch/view.rs`, kept deliberately close to it. The terminal
// solved cursor-anchored zoom, clamped panning and the fit-versus-free
// distinction in pure arithmetic over two sizes, with unit tests; porting that
// is strictly better than writing a third ad-hoc transform, and its tests port
// with it. `view.test.ts` mirrors the six Rust cases so the two stay honest.
//
// The frontend's other prior art is the asset preview, and it is the weaker
// one: it scales about the element's centre rather than the pointer, does not
// clamp, and lets the image be dragged entirely off the glass. Two details here
// are what avoid that, and both are easy to lose in a port:
//
//   - the zoom floor is recomputed from the fit on every call rather than
//     stored, so it tracks a window that has been resized; and
//   - the scale is clamped *before* the ratio is derived, so an over-scroll is
//     a transform that does nothing rather than an over-zoom followed by a snap.
//
// Pure arithmetic, no DOM: the transform applies to the element, not to the
// pixels, which is what lets tiles keep arriving into the same canvas at the
// same coordinates while the view is being moved.

/** The image rectangle in window pixels: the top-left corner, then the size. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A width and a height, of the picture or of the glass it sits on. */
export interface Size {
  w: number;
  h: number;
}

/** How far in a wheel can take the picture: window pixels per image pixel. */
export const ZOOM_CEILING = 64;

/** How far out, as a fraction of the fit scale. Below an eighth of fit the
 * picture is a stamp and the window is canvas; nothing is learned there. */
export const ZOOM_FLOOR_OF_FIT = 0.125;

/** How much of the picture a pan must leave on the glass, per axis, in window
 * pixels. Enough to grab and drag back; a picture panned entirely out of view
 * would look exactly like a render that produced nothing. */
export const MIN_VISIBLE = 32;

/** A size's own guard: every dimension is at least one before it divides. */
const at_least_1 = (n: number): number => (Number.isFinite(n) && n > 1 ? n : 1);

/** A clamp that survives an inverted range, which happens when the picture or
 * the window is smaller than the visibility floor. */
function clampLenient(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, Math.min(lo, hi)), Math.max(hi, lo));
}

/** The letterbox scale: window pixels per image pixel when the picture fits. */
export function fitScale(picture: Size, window: Size): number {
  return Math.min(
    at_least_1(window.w) / at_least_1(picture.w),
    at_least_1(window.h) / at_least_1(picture.h),
  );
}

/** The largest rectangle of `into` with the proportions of `picture`, centred. */
export function letterbox(picture: Size, into: Size): Rect {
  const scale = fitScale(picture, into);
  const w = at_least_1(picture.w) * scale;
  const h = at_least_1(picture.h) * scale;
  return {
    x: Math.max((at_least_1(into.w) - w) / 2, 0),
    y: Math.max((at_least_1(into.h) - h) / 2, 0),
    w,
    h,
  };
}

/** A view taken by hand: window pixels per image pixel, and where the image's
 * top-left corner sits in the window. `null` is the letterbox fit, recomputed
 * from the sizes at hand every time it is asked, which is what makes a resize
 * refit on its own. */
export type ViewMode = { scale: number; x: number; y: number } | null;

/** Where the picture sits, for these sizes. */
export function viewRect(mode: ViewMode, picture: Size, window: Size): Rect {
  if (mode === null) return letterbox(picture, window);
  return {
    x: mode.x,
    y: mode.y,
    w: at_least_1(picture.w) * mode.scale,
    h: at_least_1(picture.h) * mode.scale,
  };
}

/** Clamps a free offset so at least `MIN_VISIBLE` of the picture stays in the
 * window on each axis. */
function holdOnGlass(mode: ViewMode, picture: Size, window: Size): ViewMode {
  if (mode === null) return null;
  const w = at_least_1(picture.w) * mode.scale;
  const h = at_least_1(picture.h) * mode.scale;
  const ww = at_least_1(window.w);
  const wh = at_least_1(window.h);
  return {
    scale: mode.scale,
    x: clampLenient(mode.x, MIN_VISIBLE - w, ww - MIN_VISIBLE),
    y: clampLenient(mode.y, MIN_VISIBLE - h, wh - MIN_VISIBLE),
  };
}

/** Zooms by `factor` keeping the window point `cursor` over the same image
 * point, which is what makes a wheel feel anchored rather than drifting toward
 * a corner. */
export function zoomAbout(
  mode: ViewMode,
  cursor: { x: number; y: number },
  factor: number,
  picture: Size,
  window: Size,
): ViewMode {
  const rect = viewRect(mode, picture, window);
  const scale = rect.w / at_least_1(picture.w);
  const fit = fitScale(picture, window);
  const wanted = Math.min(Math.max(scale * factor, fit * ZOOM_FLOOR_OF_FIT), ZOOM_CEILING);
  // Derived after the clamp, so an over-scroll produces a transform that does
  // nothing rather than an over-zoom that then snaps back.
  const k = wanted / scale;
  return holdOnGlass(
    {
      scale: wanted,
      x: cursor.x - (cursor.x - rect.x) * k,
      y: cursor.y - (cursor.y - rect.y) * k,
    },
    picture,
    window,
  );
}

/** Moves the picture by a cursor delta in window pixels. */
export function pan(
  mode: ViewMode,
  delta: { x: number; y: number },
  picture: Size,
  window: Size,
): ViewMode {
  const rect = viewRect(mode, picture, window);
  return holdOnGlass(
    {
      scale: rect.w / at_least_1(picture.w),
      x: rect.x + delta.x,
      y: rect.y + delta.y,
    },
    picture,
    window,
  );
}

/** One image pixel to one window pixel, centred.
 *
 * The one thing here the terminal has no equivalent of: its window is sized to
 * the picture, so a hundred percent is where it starts. A dialog on a page is
 * not, so the action has to exist and has to say where it lands. Centred rather
 * than anchored anywhere, because it is a way of starting again rather than a
 * way of moving. */
export function actualSize(picture: Size, window: Size): ViewMode {
  const w = at_least_1(picture.w);
  const h = at_least_1(picture.h);
  return holdOnGlass(
    { scale: 1, x: (at_least_1(window.w) - w) / 2, y: (at_least_1(window.h) - h) / 2 },
    picture,
    window,
  );
}

/** The zoom a view is at, as window pixels per image pixel. */
export function zoomOf(mode: ViewMode, picture: Size, window: Size): number {
  return viewRect(mode, picture, window).w / at_least_1(picture.w);
}
