import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
// dockview's base CSS first; styles.css then maps every --dv-* colour onto the
// token theme, so the dock re-themes with the app and no dockview preset ships.
import "dockview-react/dist/styles/dockview.css";
import "./styles/tokens.css";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
