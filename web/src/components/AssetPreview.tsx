// The asset preview panel (item 2): opened by double-clicking an asset tile.
// Textures get a 2D pan/zoom view (pure DOM). Models get a LIVE 3D orbit on a
// second WebGPU surface: the Rust host parses the staged bytes through the
// import path, uploads to a throwaway scene, and renders on demand (open,
// orbit, zoom, resize), so an idle preview costs nothing. Nothing here touches
// the document.

import { useEffect, useRef, useState } from "react";
import { getClient } from "../engine/session";
import { pushToast } from "../store/toasts";
import { useUi } from "../store/ui";
import { assetKind } from "./AssetsPane";

function ImagePreview({ hash, name }: { hash: string; name: string }) {
  const [url, setUrl] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const dragRef = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    setZoom(1);
    setOffset({ x: 0, y: 0 });
    try {
      const bytes = getClient().assetBytes(hash);
      if (!bytes) return;
      const copy = new Uint8Array(bytes.length);
      copy.set(bytes);
      const u = URL.createObjectURL(new Blob([copy.buffer]));
      setUrl(u);
      return () => URL.revokeObjectURL(u);
    } catch {
      return;
    }
  }, [hash]);

  if (!url) return <div className="asset-preview-empty">Asset bytes unavailable.</div>;
  return (
    <div
      className="asset-preview-2d"
      onWheel={(e) => {
        e.preventDefault();
        setZoom((z) => Math.min(50, Math.max(0.05, z * Math.exp(-e.deltaY * 0.001))));
      }}
      onPointerDown={(e) => {
        dragRef.current = { x: e.clientX - offset.x, y: e.clientY - offset.y };
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (dragRef.current) {
          setOffset({ x: e.clientX - dragRef.current.x, y: e.clientY - dragRef.current.y });
        }
      }}
      onPointerUp={() => (dragRef.current = null)}
      onDoubleClick={() => {
        setZoom(1);
        setOffset({ x: 0, y: 0 });
      }}
    >
      <img
        src={url}
        alt={name}
        draggable={false}
        style={{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${zoom})` }}
      />
      <span className="asset-preview-hud">
        {Math.round(zoom * 100)}% (drag to pan, wheel to zoom, double-click to reset)
      </span>
    </div>
  );
}

function ModelPreview({ hash, name }: { hash: string; name: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState<string | null>(null);
  const dragRef = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    setError(null);
    const dpr = window.devicePixelRatio || 1;
    const size = () => {
      canvas.width = Math.max(16, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(16, Math.round(canvas.clientHeight * dpr));
    };
    size();
    try {
      getClient().previewOpen(canvas, hash, name);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      pushToast(`Model preview failed: ${msg}`, "error");
      return;
    }
    const ro = new ResizeObserver(() => {
      size();
      try {
        getClient().previewResize(canvas.width, canvas.height);
      } catch {
        /* closed */
      }
    });
    ro.observe(canvas);
    return () => {
      ro.disconnect();
      try {
        getClient().previewClose();
      } catch {
        /* not booted */
      }
    };
  }, [hash, name]);

  if (error) return <div className="asset-preview-empty">Could not preview: {error}</div>;
  return (
    <canvas
      ref={canvasRef}
      className="asset-preview-3d"
      onWheel={(e) => {
        e.preventDefault();
        try {
          getClient().previewZoom(-e.deltaY * 0.01);
        } catch {
          /* closed */
        }
      }}
      onPointerDown={(e) => {
        dragRef.current = { x: e.clientX, y: e.clientY };
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (!dragRef.current) return;
        const dx = e.clientX - dragRef.current.x;
        const dy = e.clientY - dragRef.current.y;
        dragRef.current = { x: e.clientX, y: e.clientY };
        try {
          getClient().previewOrbit(dx, dy);
        } catch {
          /* closed */
        }
      }}
      onPointerUp={() => (dragRef.current = null)}
    />
  );
}

export function AssetPreview() {
  const asset = useUi((s) => s.assetPreview);
  if (!asset) {
    return (
      <div className="asset-preview-empty">Double-click an asset in the Assets panel.</div>
    );
  }
  const kind = assetKind(asset.name);
  return (
    <div className="asset-preview">
      {kind === "image" ? (
        <ImagePreview hash={asset.hash} name={asset.name} />
      ) : kind === "model" ? (
        <ModelPreview hash={asset.hash} name={asset.name} />
      ) : (
        <div className="asset-preview-empty">
          No preview for this file type ({asset.name}).
        </div>
      )}
    </div>
  );
}
