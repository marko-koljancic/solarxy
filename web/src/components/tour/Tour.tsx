// The first-run tour: a spotlight plus a coachmark, stepping through the
// app's shape.
//
// Before this the only orientation was the empty-canvas hint ("Press Tab to
// add a node"), which is emptiness-driven rather than first-run-driven: it
// reappears whenever you delete every node, and never appears at all for
// someone opening a populated .slxy.
//
// Three rules it keeps:
//
//   - It never touches the document. A tour that builds a scene for you
//     leaves you with a scene you did not make.
//   - It skips a step whose target is not mounted, rather than spotlighting
//     empty space. Panels are dockable and a desk may omit any of them.
//   - It is replayable from Help, so dismissing it is not a one-way door.

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { usePrefs } from "../../store/prefs";
import { placeCoachmark, type Placement } from "./placement";
import { OVERVIEW_TOUR, tourById, type TourDef, type TourStep } from "./steps";

/** The overview's version, stored alongside the completion flag; bumped
 * when its steps change enough that a returning user should see it again.
 * Kept as the historical name so the prefs shape never migrates. */
export const TOUR_VERSION = OVERVIEW_TOUR.version;

const CARD = { width: 320, height: 168 };

/** Only completing the OVERVIEW marks onboarding done: replaying a topic
 * tour from Help must never eat a new user's first-run. Exported for
 * tests. */
export function completionWritesOnboarding(tourId: TourDef["id"]): boolean {
  return tourId === "overview";
}

/** The mounted targets of one tour, so a docked-away panel never strands
 * it on an empty spotlight. */
function visibleSteps(tour: TourDef): TourStep[] {
  return tour.steps.filter((s) => document.querySelector(s.target));
}

export function Tour() {
  const prefs = usePrefs((s) => s.prefs);
  const setPrefs = usePrefs((s) => s.setPrefs);
  const [tour, setTour] = useState<TourDef>(OVERVIEW_TOUR);
  const [steps, setSteps] = useState<TourStep[]>([]);
  const [i, setI] = useState(0);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const [place, setPlace] = useState<Placement | null>(null);
  const cardRef = useRef<HTMLDivElement>(null);

  const onboarding = prefs.onboarding;
  const active = steps.length > 0;

  const finish = useCallback(() => {
    setSteps([]);
    if (completionWritesOnboarding(tour.id)) {
      setPrefs({
        ...usePrefs.getState().prefs,
        onboarding: { completed: true, version: TOUR_VERSION },
      });
    }
  }, [setPrefs, tour.id]);

  // Auto-start the OVERVIEW on first run (topic tours are Help-only).
  //
  // Not a single deferred sample: the pane toolbars mount only once the
  // wasm host has reported pane rects, which can land after any fixed
  // delay (a one-shot 600ms check dropped the "Per-pane display" step on a
  // cold start). Poll until two consecutive samples agree, or give up
  // waiting for MORE steps after ~3s and run with what is mounted.
  useEffect(() => {
    const wanted = !onboarding.completed || onboarding.version < TOUR_VERSION;
    if (!wanted) return;
    let cancelled = false;
    let last = -1;
    let tries = 0;
    const tick = () => {
      if (cancelled) return;
      const found = visibleSteps(OVERVIEW_TOUR);
      tries += 1;
      if ((found.length === last && found.length > 0) || tries >= 8) {
        if (found.length > 0) {
          setTour(OVERVIEW_TOUR);
          setSteps(found);
        }
        return;
      }
      last = found.length;
      setTimeout(tick, 400);
    };
    const t = setTimeout(tick, 600);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [onboarding.completed, onboarding.version]);

  // Replay from Help: a CustomEvent whose detail names the tour; a plain
  // Event (the pre-submenu shape) falls back to the overview.
  useEffect(() => {
    const replay = (e: Event) => {
      const id = e instanceof CustomEvent ? (e.detail as { id?: unknown } | null)?.id : undefined;
      const def = tourById(id);
      setTour(def);
      setI(0);
      setSteps(visibleSteps(def));
    };
    window.addEventListener("solarxy:tour", replay);
    return () => window.removeEventListener("solarxy:tour", replay);
  }, []);

  const step = steps[i];

  // Measure the target, then place the card against the measured card box.
  useLayoutEffect(() => {
    if (!step) return;
    const el = document.querySelector(step.target);
    if (!el) return;
    const a = el.getBoundingClientRect();
    setAnchor(a);
    const card = cardRef.current?.getBoundingClientRect();
    setPlace(
      placeCoachmark(
        { left: a.left, top: a.top, width: a.width, height: a.height },
        card && card.height > 0 ? card : CARD,
        { width: window.innerWidth, height: window.innerHeight },
        step.side,
      ),
    );
  }, [step, i]);

  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        finish();
      } else if (e.key === "ArrowRight" || e.key === "Enter") {
        e.preventDefault();
        setI((n) => (n + 1 < steps.length ? n + 1 : (finish(), n)));
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        setI((n) => Math.max(0, n - 1));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, steps.length, finish]);

  if (!active || !step || !anchor) return null;

  const next = () => (i + 1 < steps.length ? setI(i + 1) : finish());

  return createPortal(
    <div className="tour-layer" role="dialog" aria-label="Getting started">
      {/* The scrim is four rects around the target, not one box-shadow: the
       * hole must not intercept pointer events, so the app stays live under
       * it and the spotlight reads as a hole rather than a highlight. */}
      <div className="tour-scrim" style={{ inset: `0 0 auto 0`, height: anchor.top }} />
      <div className="tour-scrim" style={{ top: anchor.bottom, bottom: 0, left: 0, right: 0 }} />
      <div
        className="tour-scrim"
        style={{ top: anchor.top, height: anchor.height, left: 0, width: anchor.left }}
      />
      <div
        className="tour-scrim"
        style={{ top: anchor.top, height: anchor.height, left: anchor.right, right: 0 }}
      />
      <div
        className="tour-spot"
        style={{
          left: anchor.left,
          top: anchor.top,
          width: anchor.width,
          height: anchor.height,
        }}
      />

      <div
        ref={cardRef}
        className={`tour-card tour-card-${place?.side ?? "bottom"}`}
        style={place ? { left: place.left, top: place.top } : { visibility: "hidden" }}
      >
        <div className="tour-card-title">{step.title}</div>
        <p className="tour-card-body">{step.body}</p>
        <div className="tour-card-foot">
          <span className="tour-progress">
            {i + 1} of {steps.length}
          </span>
          <span className="spacer" />
          <button className="btn" onClick={finish}>
            Skip
          </button>
          {i > 0 && (
            <button className="btn" onClick={() => setI(i - 1)}>
              Back
            </button>
          )}
          <button className="btn primary" onClick={next}>
            {i + 1 < steps.length ? "Next" : "Done"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
