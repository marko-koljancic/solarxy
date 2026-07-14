import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportBoundaryError } from "../telemetry";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/** Catches a React render/effect crash, reports it, and shows a recovery screen
 * instead of a blank page.
 *
 * This is ONE of four crash surfaces (see `telemetry.ts`): it cannot see a wasm
 * panic from the rAF frame pump, an unhandled rejection, or anything in the
 * import worker. Those are covered by global handlers and by the worker itself.
 *
 * Reload is offered rather than "try again": the wasm engine holds all document
 * state, and a React crash tells us nothing about whether that state is still
 * coherent. Re-rendering on top of a possibly-corrupt engine would turn one bug
 * report into two. The document is autosaved to OPFS, so a reload recovers it. */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    reportBoundaryError(error, info.componentStack ?? "");
  }

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="fatal-screen" role="alert">
        <div className="fatal-card">
          <h1>Solarxy hit a problem</h1>
          <p>
            Something broke badly enough that the interface could not carry on. The error has
            been reported.
          </p>
          <p className="fatal-detail">
            Your work is autosaved. Reloading should offer to recover it.
          </p>
          <pre className="fatal-message">{error.message}</pre>
          <button type="button" onClick={() => window.location.reload()}>
            Reload Solarxy
          </button>
        </div>
      </div>
    );
  }
}
