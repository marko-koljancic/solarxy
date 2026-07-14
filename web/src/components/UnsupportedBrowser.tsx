/** Shown when the browser has no WebGPU.
 *
 * Rendered INSTEAD of the app, and deliberately before anything touches the wasm:
 * `main.tsx` checks `navigator.gpu` before it mounts `<App/>`, and the engine is
 * only loaded from inside the app tree. So a browser that cannot run Solarxy
 * never downloads the 3.4 MB wasm binary to be told so.
 *
 * There is no separate landing page (the app is served at the domain root), so
 * this IS the unsupported-browser page. */
export function UnsupportedBrowser() {
  return (
    <div className="fatal-screen" role="alert">
      <div className="fatal-card">
        <h1>Solarxy Web needs WebGPU</h1>
        <p>
          This browser does not support WebGPU, the graphics API Solarxy renders with. Nothing
          has been downloaded beyond this page.
        </p>

        <h2>What works today</h2>
        <ul className="fatal-list">
          <li>
            <strong>Chrome</strong> or <strong>Edge</strong> 113+, on Windows, macOS or Linux.
          </li>
          <li>
            <strong>Safari</strong> 26+ on macOS.
          </li>
          <li>
            <strong>Firefox</strong> support is arriving; it varies by platform and version.
          </li>
        </ul>

        <p className="fatal-detail">
          On Linux, WebGPU may need to be enabled explicitly. On older hardware or in a virtual
          machine, the browser may disable it even where it is otherwise supported.
        </p>

        <h2>Or use the desktop app</h2>
        <p>
          Solarxy also ships as a native viewer and validator for macOS, Windows and Linux, with
          no browser requirement.
        </p>
        <p>
          <a href="https://github.com/marko-koljancic/solarxy/releases">Downloads</a>
          {" · "}
          <a href="https://github.com/marko-koljancic/solarxy/wiki">Documentation</a>
        </p>
      </div>
    </div>
  );
}
