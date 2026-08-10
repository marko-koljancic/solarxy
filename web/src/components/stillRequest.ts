// What a `render` node's action asks the host to render.
//
// Extracted from the parameter panel so the mapping is testable on its own:
// the quality preset's sample count and the engine's spelling are the two
// places this can silently disagree with the Rust side, and both are pure
// functions of the node.
//
// Every value is read the way a registry-driven panel reads anything: the
// node's literal if it has one, else the descriptor's default. A node saved
// before version 2 has no literal for the new keys, so this is also what makes
// an old document render with the new defaults rather than with zeroes.

import { descriptorFor } from "../registry/datatypes";
import type { NodeMirror, RegistrySnapshot } from "../engine/types";
import type { StillRenderRequest } from "./StillRenderModal";

/** The sample count each quality preset means.
 *
 * Mirrors the enum the `render` node declares. A preset this does not know
 * falls back to Good, which is what a registry-driven panel does with anything
 * it has not been taught: the node stays authoritative and the UI degrades
 * rather than throwing. */
export const QUALITY_SAMPLES: Record<string, number> = {
  draft: 16,
  good: 64,
  high: 256,
  reference: 1024,
};

function defaultOf(registry: RegistrySnapshot | null, typeId: string, key: string): unknown {
  return descriptorFor(registry, typeId)?.params.find((p) => p.key === key)?.default;
}

function literalOf(node: NodeMirror, key: string): unknown {
  const src = node.params[key];
  return src && src.kind === "literal" ? (src as { value: unknown }).value : undefined;
}

export function numberParam(
  node: NodeMirror,
  registry: RegistrySnapshot | null,
  key: string,
  fallback: number,
): number {
  const literal = literalOf(node, key);
  if (literal !== undefined) return Number(literal) || fallback;
  return Number(defaultOf(registry, node.typeId, key)) || fallback;
}

export function enumParam(
  node: NodeMirror,
  registry: RegistrySnapshot | null,
  key: string,
  fallback: string,
): string {
  const literal = literalOf(node, key);
  if (literal !== undefined) return String(literal) || fallback;
  const d = defaultOf(registry, node.typeId, key);
  return typeof d === "string" ? d : fallback;
}

export function boolParam(
  node: NodeMirror,
  registry: RegistrySnapshot | null,
  key: string,
  fallback: boolean,
): boolean {
  const literal = literalOf(node, key);
  if (literal !== undefined) return Boolean(literal);
  const d = defaultOf(registry, node.typeId, key);
  return typeof d === "boolean" ? d : fallback;
}

/** The request a `render` node's action produces.
 *
 * `camera` is the node's `camera_path` value, or null when it names none, in
 * which case the host shoots the active pane's current view. Either way the
 * viewport does not move: a shot is a property of the scene. */
export function stillRequestFor(
  node: NodeMirror,
  registry: RegistrySnapshot | null,
  camera: number | null,
): StillRenderRequest {
  return {
    width: numberParam(node, registry, "width", 1920),
    height: numberParam(node, registry, "height", 1080),
    samples: QUALITY_SAMPLES[enumParam(node, registry, "quality", "good")] ?? 64,
    engine: enumParam(node, registry, "engine", "raster") === "traced" ? "pathTraced" : "raster",
    denoise: boolParam(node, registry, "denoise", false),
    camera,
  };
}
