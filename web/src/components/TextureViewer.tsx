// The texture viewer pane (context-expansion phase 19): a live 2D view of
// the texture network's published image. Pull-based: the pane fetches the
// pixels from the host when the cook state changes (cooked images never
// ride the event stream), draws them into a plain 2D canvas, and letterboxes
// with CSS. Shows the network whose canvas is open in the Nodes pane when
// that canvas is a texture network, else the first texture network at root.

import { useEffect, useMemo, useRef, useState } from "react";
import { getClient } from "../engine/session";
import { contextKind, descriptorFor } from "../registry/datatypes";
import { nodeLabel } from "../flow/nodeLabel";
import { selectGraph, useMirror } from "../store/mirror";

export function TextureViewer() {
  const current = useMirror((s) => s.current);
  const registry = useMirror((s) => s.registry);
  const rootNodes = useMirror((s) => selectGraph(s, "root").nodes);
  // The cook record's identity changes on every applied cook batch, which
  // is exactly the refetch signal (over-fetching on unrelated cooks is
  // cheap next to a texture decode and keeps this a pure mirror consumer).
  const cook = useMirror((s) => s.cook);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [dims, setDims] = useState<[number, number] | null>(null);

  // The network to preview: the open canvas when it IS a texture network,
  // else the first texnet-like container at root.
  const owner = useMemo(() => {
    if (current !== "root" && contextKind(registry, current, rootNodes) === "tex") {
      return current.subflow;
    }
    return (
      rootNodes.find((n) => descriptorFor(registry, n.typeId)?.opens === "tex")?.id ?? null
    );
  }, [current, registry, rootNodes]);

  const ownerNode = rootNodes.find((n) => n.id === owner);
  const title = ownerNode ? nodeLabel(ownerNode, descriptorFor(registry, ownerNode.typeId)) : null;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (owner == null || !canvas) {
      setDims(null);
      return;
    }
    const preview = getClient().texturePreview(owner);
    if (!preview) {
      setDims(null);
      return;
    }
    canvas.width = preview.width;
    canvas.height = preview.height;
    const ctx2d = canvas.getContext("2d");
    if (!ctx2d) return;
    ctx2d.putImageData(
      new ImageData(new Uint8ClampedArray(preview.pixels), preview.width, preview.height),
      0,
      0,
    );
    setDims([preview.width, preview.height]);
  }, [owner, cook]);

  return (
    <div className="texture-viewer">
      <div className="texture-viewer-status">
        {owner == null
          ? "No texture network in the scene."
          : dims
            ? `${title ?? "Texture"} · ${dims[0]} × ${dims[1]}`
            : `${title ?? "Texture"} · no image published (set a display node)`}
      </div>
      <div className="texture-viewer-stage">
        <canvas
          ref={canvasRef}
          className="texture-viewer-canvas"
          style={{ display: dims ? "block" : "none" }}
        />
      </div>
    </div>
  );
}
