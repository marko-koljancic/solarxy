import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "dockview-react/dist/styles/dockview.css";
import "./styles.css";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("no #root");
// StrictMode is deliberate: it double-mounts every effect in dev, which is
// exactly the case that would break a naive canvas-adoption effect.
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
