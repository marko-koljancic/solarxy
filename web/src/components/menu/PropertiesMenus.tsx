// The Properties panel's own menu bar (Stage 7, feedback item 7): a Node
// menu (info, path, bypass, display flag) and a Params menu (reset all /
// reset tab). Pure interpreters of the mirror and registry; every
// mutation is a Command dispatch, and the reset command is one undo step
// engine-side.

import { useRef } from "react";
import { toggleMaximize } from "../../dock/api";
import { dispatch } from "../../engine/session";
import { nodePathOf, openNodeInfo, setDisplayFlag, toggleBypass } from "../../flow/nodeActions";
import { descriptorFor } from "../../registry/datatypes";
import { selectGraph, useMirror } from "../../store/mirror";
import { pushToast } from "../../store/toasts";
import { useUi } from "../../store/ui";
import {
  groupKeys,
  paramTabs,
  resolveActiveTab,
  tabLabel,
  VALIDATION_TAB,
} from "../paramVisibility";
import { MenuItem, type MenuEntry } from "./MenuItem";

export function PropertiesMenuBar() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const reports = useMirror((s) => s.reports);
  const storedTab = useUi((s) => s.paramTab);
  const barRef = useRef<HTMLElement>(null);

  const node = graph.nodes.find((n) => n.id === graph.selection[0]);
  const desc = node ? descriptorFor(registry, node.typeId) : undefined;
  const params = desc?.params ?? [];
  const tabs = node ? paramTabs(params, Boolean(reports[node.id])) : [];
  const active = resolveActiveTab(tabs, storedTab);
  const resettableTab = active !== undefined && active !== VALIDATION_TAB;

  const nodeEntries: MenuEntry[] = [
    {
      label: "Node Info",
      shortcut: "I",
      disabled: !node,
      onClick: () => {
        if (!node) return;
        const r = barRef.current?.getBoundingClientRect();
        openNodeInfo(node.id, current, (r?.left ?? 80) + 16, (r?.bottom ?? 80) + 16);
      },
    },
    {
      label: "Copy Node Path",
      disabled: !node,
      onClick: () => {
        if (!node) return;
        const path = nodePathOf(current, node);
        void navigator.clipboard?.writeText(path);
        pushToast(`Copied ${path}`);
      },
    },
    { divider: true },
    {
      label: "Toggle Bypass",
      shortcut: "B",
      disabled: !node || desc?.bypass.mode === "notBypassable",
      onClick: () => node && toggleBypass(current, node),
    },
    {
      label: "Set Display Flag",
      disabled: !node || current === "root",
      onClick: () => node && setDisplayFlag(current, node.id),
    },
  ];

  const paramsEntries: MenuEntry[] = [
    {
      label: "Reset All Parameters",
      disabled: !node || params.length === 0,
      onClick: () => node && dispatch({ type: "resetParams", ctx: current, node: node.id }),
    },
    {
      label: resettableTab && active ? `Reset ${tabLabel(active)} Tab` : "Reset Current Tab",
      disabled: !node || !resettableTab,
      onClick: () => {
        if (!node || !active) return;
        dispatch({
          type: "resetParams",
          ctx: current,
          node: node.id,
          keys: groupKeys(params, active),
        });
      },
    },
  ];

  // Panel chrome, not node state: the same role "View" plays on the Viewport
  // and Nodes bars. Kept separate from the Node menu on purpose -- that one
  // acts on the selected node, this one acts on the panel around it.
  const viewEntries: MenuEntry[] = [
    {
      label: "Maximize Panel",
      shortcut: "Esc to restore",
      onClick: () => toggleMaximize("properties"),
    },
  ];

  return (
    <nav ref={barRef} className="menu-bar properties-menu-bar">
      <MenuItem title="Node" entries={nodeEntries} />
      <MenuItem title="Params" entries={paramsEntries} />
      <MenuItem title="View" entries={viewEntries} />
    </nav>
  );
}
