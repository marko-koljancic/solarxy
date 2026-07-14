// Crash reporting.
//
// A React error boundary is NOT enough here, and that is the whole reason this
// module exists rather than a single <ErrorBoundary> wrapper. Solarxy Web can die
// in four distinct places, and a boundary only sees one of them:
//
//   1. React render / effects        -> the boundary catches it.
//   2. The rAF frame pump            -> a wasm panic (or any throw) inside
//                                       requestAnimationFrame unwinds OUTSIDE
//                                       React. The boundary never sees it.
//                                       `window.onerror` does.
//   3. Rejected promises             -> async work in the session pump (imports,
//                                       HDRI prep, screenshot readback) rejects
//                                       with nobody awaiting. `unhandledrejection`.
//   4. The import Web Worker         -> a SECOND wasm instance, in another realm.
//                                       Neither the boundary nor window.onerror
//                                       can see it; the worker reports for itself.
//
// Rust panics reach (2) or (4): `console_error_panic_hook` logs the panic message
// to console.error and then the wasm traps, which surfaces to JS as a
// `RuntimeError: unreachable`. That throw is what we actually capture. The panic
// text is in the console, and we attach the last console.error to give the report
// something legible to sit next to the opaque trap.
//
// The DSN is public by design: it identifies the project for SENDING events and
// grants no read access, which is why it is committed rather than kept secret.

import * as Sentry from "@sentry/browser";

const DSN = import.meta.env.VITE_GLITCHTIP_DSN ?? "";

/** The most recent `console.error`, attached to reports as context. A wasm trap
 * arrives as a bare `RuntimeError: unreachable`; the panic message that explains
 * it went to the console a moment earlier, and without this the report is
 * unactionable. */
let lastConsoleError: string | null = null;

function captureConsoleErrors(): void {
  const original = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    try {
      lastConsoleError = args
        .map((a) => (a instanceof Error ? `${a.name}: ${a.message}` : String(a)))
        .join(" ")
        .slice(0, 4000);
    } catch {
      // Never let instrumentation break logging.
    }
    original(...args);
  };
}

/** Installs crash reporting. Safe to call when no DSN is configured: it becomes
 * a no-op rather than throwing, so a local build with no `.env` still runs. */
export function initTelemetry(): void {
  captureConsoleErrors();

  if (!DSN) {
    // Local dev, or a build with no DSN. Not an error.
    return;
  }

  Sentry.init({
    dsn: DSN,
    release: `solarxy-web@${__APP_VERSION__}`,
    // Errors only. No performance tracing, no session replay: this is a
    // single-maintainer free tier and a 3D app would flood both.
    tracesSampleRate: 0,
    // The app is entirely client-side and holds no accounts, so there is no user
    // to identify and nothing to scrub. Send no IP.
    sendDefaultPii: false,
    beforeSend(event) {
      if (lastConsoleError) {
        event.extra = { ...event.extra, lastConsoleError };
      }
      return event;
    },
  });

  // (2) and (3). Sentry's browser SDK installs global handlers for both, but they
  // are declared explicitly here so the four-surface contract above is legible in
  // code rather than being an implicit property of the SDK's defaults.
  window.addEventListener("error", (e) => {
    Sentry.captureException(e.error ?? new Error(e.message), {
      tags: { surface: "window.onerror" },
    });
  });
  window.addEventListener("unhandledrejection", (e) => {
    const reason: unknown = e.reason;
    Sentry.captureException(reason instanceof Error ? reason : new Error(String(reason)), {
      tags: { surface: "unhandledrejection" },
    });
  });
}

/** Reports an error from the React error boundary (surface 1). */
export function reportBoundaryError(error: Error, componentStack: string): void {
  if (!DSN) return;
  Sentry.captureException(error, {
    tags: { surface: "react" },
    contexts: { react: { componentStack } },
  });
}

/** Reports an error the import worker forwarded to the main thread (surface 4).
 * The worker runs a second wasm instance in its own realm, so nothing on this
 * thread can observe its failures; it has to tell us. */
export function reportWorkerError(message: string, stack?: string): void {
  if (!DSN) return;
  const err = new Error(message);
  if (stack) err.stack = stack;
  Sentry.captureException(err, { tags: { surface: "import-worker" } });
}

/** Deliberate crash triggers, for the release smoke test. Reachable only from the
 * console; nothing in the UI calls them. Each one exercises a DIFFERENT surface,
 * because "errors reach GlitchTip" is four separate claims, not one. */
export function installSmokeHooks(): void {
  (window as unknown as Record<string, unknown>).__solarxyCrash = {
    react: () => {
      throw new Error("[smoke] deliberate React error");
    },
    async: () => {
      void Promise.reject(new Error("[smoke] deliberate unhandled rejection"));
    },
    global: () => {
      setTimeout(() => {
        throw new Error("[smoke] deliberate window.onerror (rAF-like surface)");
      }, 0);
    },
  };
}
