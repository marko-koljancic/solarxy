// The four dock panels. Each is a thin wrapper around a pane component that
// already made self-contained, which is exactly why this phase does not
// have to restructure any of them.

import {
  DockviewDefaultTab,
  type IDockviewHeaderActionsProps,
  type IDockviewPanelHeaderProps,
  type IDockviewPanelProps,
} from "dockview-react";
import { useEffect, useRef, useState } from "react";
import { toggleMaximize } from "./api";
import { clearHoveredPanel, setHoveredPanel } from "./hover";
import { IconMaximize, IconRestore } from "../icons";
import { AssetPreview } from "../components/AssetPreview";
import { AssetsPane } from "../components/AssetsPane";
import { AttributesPane } from "../components/AttributesPane";
import { NodePane } from "../components/NodePane";
import { ParameterPanel } from "../components/ParameterPanel";
import { PropertiesMenuBar } from "../components/menu/PropertiesMenus";
import { Viewport } from "../components/Viewport";
import { ReviewPanel } from "../components/review/ReviewPanel";
import { TextureViewer } from "../components/TextureViewer";
import { TreePane } from "../components/TreePane";
import { nodeLabel } from "../flow/nodeLabel";
import { contextKind, descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { useUi } from "../store/ui";

/** The selected node's display label (its `name` param when renamed, else the
 * type display name), shown in the Properties header. */
function useSelectedNodeName(): string {
  const registry = useMirror((s) => s.registry);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const selected = graph.nodes.find((n) => n.id === graph.selection[0]);
  if (!selected) return "";
  return nodeLabel(selected, descriptorFor(registry, selected.typeId));
}

/** Records which panel the pointer is over (dock/hover.ts) so the
 * panel-maximize shortcut can act on the hovered panel. Wraps every panel
 * body, so the tracking follows the content through grid, floating and
 * maximized states alike. */
function HoverTracked({ id, children }: { id: string; children: React.ReactNode }) {
  useEffect(() => () => clearHoveredPanel(id), [id]);
  return (
    <div
      className="dock-panel-hover"
      onPointerEnter={() => setHoveredPanel(id)}
      onPointerLeave={() => clearHoveredPanel(id)}
    >
      {children}
    </div>
  );
}

function ViewportPanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <Viewport />
    </HoverTracked>
  );
}

function NodesPanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <NodePane />
    </HoverTracked>
  );
}

/** The single Properties panel. It replaces the bottom drawer and the
 * right-docked column: dockview owns the docking, resizing and tabbing that
 * those two hand-rolled variants existed to provide. */
function PropertiesPanel(props: IDockviewPanelProps) {
  const title = useSelectedNodeName();
  return (
    <HoverTracked id={props.api.id}>
      <div className="properties-panel">
        <PropertiesMenuBar />
        {title && <div className="properties-panel-context">{title}</div>}
        <div className="properties-panel-body">
          <ParameterPanel />
        </div>
      </div>
    </HoverTracked>
  );
}

function ReviewDockPanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <ReviewPanel />
    </HoverTracked>
  );
}

function AssetsPanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <AssetsPane />
    </HoverTracked>
  );
}

function AssetPreviewPanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <AssetPreview />
    </HoverTracked>
  );
}

function TexturePanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <TextureViewer />
    </HoverTracked>
  );
}

function AttributesPanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <AttributesPane />
    </HoverTracked>
  );
}

function TreePanel(props: IDockviewPanelProps) {
  return (
    <HoverTracked id={props.api.id}>
      <TreePane />
    </HoverTracked>
  );
}

export const DOCK_COMPONENTS = {
  viewport: ViewportPanel,
  nodes: NodesPanel,
  properties: PropertiesPanel,
  review: ReviewDockPanel,
  assets: AssetsPanel,
  assetPreview: AssetPreviewPanel,
  texture: TexturePanel,
  attributes: AttributesPanel,
  tree: TreePanel,
};

// Item 4: per-pane header tint (Houdini-style). A curated pastel set: the four
// node-category pastels plus extras, so many open panes stay distinguishable.
// Header/tab only; the pastels are light in both themes and pair with dark ink.
const PANE_PALETTE = [
  "#c9dcf2", // blue (generators)
  "#c8e8d6", // green (topology)
  "#ddd0ee", // purple (import)
  "#eed9c9", // tan (utility)
  "#c9c2f0", // lavender
  "#f2c9a0", // peach
  "#f2c9d6", // pink
  "#b8e3e0", // teal
  "#eee3b0", // yellow
  "#f0c8b8", // coral
] as const;

/** The tint swatch popover, opened by right-clicking a pane tab. Fixed at the
 * click point; closes on outside pointerdown, Esc, or a pick. */
function PaneColorPicker({
  id,
  x,
  y,
  onClose,
}: {
  id: string;
  x: number;
  y: number;
  onClose: () => void;
}) {
  const current = useUi((s) => s.paneColors[id]);
  const setPaneColor = useUi((s) => s.setPaneColor);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      if (!(e.target instanceof Element) || !ref.current?.contains(e.target)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  return (
    <div ref={ref} className="pane-color-picker" style={{ position: "fixed", left: x, top: y }}>
      <button
        type="button"
        className={`pane-swatch pane-swatch-none${current ? "" : " active"}`}
        title="No color"
        aria-label="No color"
        onClick={() => {
          setPaneColor(id, null);
          onClose();
        }}
      />
      {PANE_PALETTE.map((c) => (
        <button
          key={c}
          type="button"
          className={`pane-swatch${current === c ? " active" : ""}`}
          style={{ background: c }}
          title={c}
          aria-label={c}
          onClick={() => {
            setPaneColor(id, c);
            onClose();
          }}
        />
      ))}
    </div>
  );
}

/** A dockview tab wrapped so its header carries the per-pane tint and a
 * right-click color picker. Preserves the default tab's drag / close behavior
 * (we only wrap it, never reimplement it). */
/** The automatic per-context-kind tint: the Nodes tab
 * reflects which network kind its canvas shows, and the Texture viewer
 * carries the image family's pink. A manual right-click tint wins. */
function autoPaneColor(id: string, kind: ReturnType<typeof contextKind>): string | undefined {
  if (id === "texture") return "#f2c9d6";
  if (id !== "nodes") return undefined;
  if (kind === "tex") return "#f2c9d6";
  if (kind === "mat") return "#c9c2f0";
  return undefined;
}

function ColoredTabInner({
  props,
  hideClose,
}: {
  props: IDockviewPanelHeaderProps;
  hideClose: boolean;
}) {
  const id = props.api.id;
  const manual = useUi((s) => s.paneColors[id]);
  const current = useMirror((s) => s.current);
  const registry = useMirror((s) => s.registry);
  const rootNodes = useMirror((s) => selectGraph(s, "root").nodes);
  const color = manual ?? autoPaneColor(id, contextKind(registry, current, rootNodes));
  const [picker, setPicker] = useState<{ x: number; y: number } | null>(null);
  return (
    <div
      className={`pane-tab-wrap${color ? " tinted" : ""}`}
      style={color ? ({ "--pane-tint": color } as React.CSSProperties) : undefined}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setPicker({ x: e.clientX, y: e.clientY });
      }}
    >
      <DockviewDefaultTab {...props} hideClose={hideClose} />
      {picker && (
        <PaneColorPicker id={id} x={picker.x} y={picker.y} onClose={() => setPicker(null)} />
      )}
    </div>
  );
}

/** The default (closeable) tab: colorable. */
function ColoredTab(props: IDockviewPanelHeaderProps) {
  return <ColoredTabInner props={props} hideClose={false} />;
}

/** The pinned tab: no close button, still colorable. The other half of the pin
 * (cancelling the tab drag) lives in Dock.tsx; together they mirror the desktop,
 * where the Viewport dock tab is neither floatable nor closeable. */
function PinnedTab(props: IDockviewPanelHeaderProps) {
  return <ColoredTabInner props={props} hideClose />;
}

export const DOCK_TAB_COMPONENTS = {
  pinned: PinnedTab,
  colored: ColoredTab,
};

/** The maximize / restore toggle at the right end of every tab strip, next to
 * the active tab's close button. Group-level on purpose: dockview maximize is
 * a group operation, and this is also what gives the pinned Viewport tab (which
 * has no close button) its maximize control. Grid groups only, maximize has no
 * meaning for floating or popout groups. */
export function MaximizeHeaderAction(props: IDockviewHeaderActionsProps) {
  // isMaximized() is not observable state; re-render on the container event.
  const [, setTick] = useState(0);
  useEffect(() => {
    const sub = props.containerApi.onDidMaximizedGroupChange(() => setTick((t) => t + 1));
    return () => sub.dispose();
  }, [props.containerApi]);

  if (props.group.api.location.type !== "grid") return null;
  const panel = props.activePanel;
  if (!panel) return null;
  const maximized = panel.api.isMaximized();
  return (
    <div className="dock-header-actions">
      <button
        type="button"
        className="dock-header-action"
        title={maximized ? "Restore panel (` or Esc)" : "Maximize panel (`)"}
        aria-label={maximized ? "Restore panel" : "Maximize panel"}
        onClick={() => toggleMaximize(panel.id)}
      >
        {maximized ? <IconRestore size={13} /> : <IconMaximize size={13} />}
      </button>
    </div>
  );
}
