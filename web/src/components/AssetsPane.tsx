// The Assets pane: a thumbnail grid of every staged asset in the
// scene, from the engine's authoritative manifest (view and preview only; no
// management). Textures show real decoded thumbnails; models show typed
// glyphs. Double-click opens the preview panel (2D pan/zoom for textures,
// live 3D orbit for models).

import { useEffect, useMemo, useState } from "react";
import { openAssetPreviewPanel } from "../dock/api";
import { getClient, isBooted } from "../engine/session";
import type { AssetRef } from "../engine/types";
import { useMirror } from "../store/mirror";
import { useUi } from "../store/ui";

export type AssetKind = "image" | "model" | "hdri" | "other";

export function assetKind(name: string): AssetKind {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "webp"].includes(ext)) return "image";
  if (["obj", "gltf", "glb", "stl", "ply"].includes(ext)) return "model";
  if (["hdr", "exr"].includes(ext)) return "hdri";
  return "other";
}

/** Session-lived object-URL cache for texture thumbnails, keyed by content
 * hash (content-addressed, so a hash never changes meaning). */
const thumbCache = new Map<string, string>();

function thumbUrl(hash: string): string | undefined {
  const cached = thumbCache.get(hash);
  if (cached) return cached;
  try {
    const bytes = getClient().assetBytes(hash);
    if (!bytes) return undefined;
    const copy = new Uint8Array(bytes.length);
    copy.set(bytes);
    const url = URL.createObjectURL(new Blob([copy.buffer]));
    thumbCache.set(hash, url);
    return url;
  } catch {
    return undefined;
  }
}

const KIND_LABEL: Record<AssetKind, string> = {
  image: "Texture",
  model: "Model",
  hdri: "HDRI",
  other: "File",
};

function ModelGlyph() {
  return (
    <svg width="40" height="40" viewBox="0 0 24 24" aria-hidden>
      <path
        d="M12 2 L21 7 V17 L12 22 L3 17 V7 Z M12 2 V12 M3 7 L12 12 L21 7"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function FileGlyph() {
  return (
    <svg width="40" height="40" viewBox="0 0 24 24" aria-hidden>
      <path
        d="M6 2 H14 L18 6 V22 H6 Z M14 2 V6 H18"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function AssetsPane() {
  // The manifest is engine truth; refresh whenever the document advances
  // (imports stage assets through commands, so revision tracks it).
  const revision = useMirror((s) => s.revision);
  const [assets, setAssets] = useState<AssetRef[]>([]);
  useEffect(() => {
    if (!isBooted()) return;
    try {
      setAssets(getClient().assetManifest());
    } catch {
      setAssets([]);
    }
  }, [revision]);

  const sorted = useMemo(
    () => [...assets].sort((a, b) => a.name.localeCompare(b.name)),
    [assets],
  );

  const open = (asset: AssetRef) => {
    useUi.getState().setAssetPreview({ hash: asset.hash, name: asset.name });
    openAssetPreviewPanel(`Preview: ${asset.name}`);
  };

  if (sorted.length === 0) {
    return (
      <div className="assets-pane assets-empty">
        <p>No assets staged yet.</p>
        <p className="assets-empty-hint">
          Drop a model or add an Import node; its files appear here.
        </p>
      </div>
    );
  }

  return (
    <div className="assets-pane">
      <div className="assets-grid">
        {sorted.map((a) => {
          const kind = assetKind(a.name);
          const thumb = kind === "image" ? thumbUrl(a.hash) : undefined;
          return (
            <button
              key={a.hash}
              type="button"
              className="asset-tile"
              title={`${a.name}\n${a.hash.slice(0, 12)}...  (double-click to preview)`}
              onDoubleClick={() => open(a)}
            >
              <span className="asset-thumb">
                {thumb ? (
                  <img src={thumb} alt={a.name} loading="lazy" />
                ) : kind === "model" ? (
                  <ModelGlyph />
                ) : (
                  <FileGlyph />
                )}
              </span>
              <span className="asset-name">{a.name}</span>
              <span className="asset-kind">{KIND_LABEL[kind]}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
