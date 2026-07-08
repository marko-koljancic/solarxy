// The wasm boundary client: loads the wasm module, constructs the
// `SolarxyApp`, and exposes typed wrappers over its `any`-typed methods.
// The React app holds exactly one instance.

import init, { SolarxyApp, start } from "../wasm/pkg/solarxy_web.js";
import wasmUrl from "../wasm/pkg/solarxy_web_bg.wasm?url";
import type {
  Command,
  DocumentSnapshot,
  EventBatch,
  GraphContext,
  ImportJob,
  NodeId,
  ParamSource,
  RegistrySnapshot,
  SaveExtra,
  SlxyLoadResult,
} from "./types";

export class SolarxyClient {
  private constructor(private readonly app: SolarxyApp) {}

  /** Loads the wasm, installs the panic hook, and boots over a canvas. */
  static async create(canvas: HTMLCanvasElement): Promise<SolarxyClient> {
    await init({ module_or_path: wasmUrl });
    start(); // panic hook -> console.error (idempotent)
    const app = await SolarxyApp.create(canvas);
    return new SolarxyClient(app);
  }

  /** Applies one command; returns the event batch for the mirror. */
  dispatch(cmd: Command): EventBatch {
    return this.app.dispatch(cmd) as EventBatch;
  }

  /** Cooks + renders one frame; returns the cook event batch. */
  frame(dtMs: number): EventBatch {
    return this.app.frame(dtMs) as EventBatch;
  }

  previewParam(ctx: GraphContext, node: NodeId, key: string, value: ParamSource): void {
    this.app.preview_param(ctx, node, key, value);
  }

  /** Rust-side pick: canvas pixel -> producing geo node id (or undefined). */
  pick(x: number, y: number): NodeId | undefined {
    return this.app.pick(x, y);
  }

  snapshot(): DocumentSnapshot {
    return this.app.snapshot() as DocumentSnapshot;
  }

  registrySnapshot(): RegistrySnapshot {
    return this.app.registry_snapshot() as RegistrySnapshot;
  }

  saveScene(): unknown {
    return this.app.save_scene();
  }

  loadScene(file: unknown): EventBatch {
    return this.app.load_scene(file) as EventBatch;
  }

  copyNodes(ctx: GraphContext, ids: NodeId[]): unknown {
    return this.app.copy_nodes(ctx, new Float64Array(ids));
  }

  resize(width: number, height: number): void {
    this.app.resize(width, height);
  }

  orbit(dx: number, dy: number): void {
    this.app.orbit(dx, dy);
  }

  pan(dx: number, dy: number): void {
    this.app.pan(dx, dy);
  }

  dolly(amount: number): void {
    this.app.dolly(amount);
  }

  /** The ids of currently stale (dirty) nodes. */
  staleNodes(): number[] {
    return Array.from(this.app.stale_nodes() as Float64Array);
  }

  nodeTypeCount(): number {
    return this.app.node_type_count();
  }

  // ---- Phase 5: asset staging + the import-worker pump ----

  /** Stages asset bytes; returns the content id the import node references. */
  stageAsset(name: string, mime: string, sha256: string, bytes: Uint8Array): string {
    return this.app.stage_asset(name, mime, sha256, bytes) as string;
  }

  /** A fresh copy of the staged bytes for a hash (undefined if absent). */
  assetBytes(hash: string): Uint8Array | undefined {
    return this.app.asset_bytes(hash) as Uint8Array | undefined;
  }

  /** Drains the import jobs the last cook spawned (to run in the worker). */
  takeImportJobs(): ImportJob[] {
    return this.app.take_import_jobs() as ImportJob[];
  }

  /** Commits a worker-parsed model (transfer blob) under the generation guard. */
  submitParsedModel(ctx: GraphContext, jobId: number, blob: Uint8Array): EventBatch {
    return this.app.submit_parsed_model(ctx, jobId, blob) as EventBatch;
  }

  /** Reports a worker parse failure (the node badges the error). */
  submitParseError(ctx: GraphContext, jobId: number, message: string): EventBatch {
    return this.app.submit_parse_error(ctx, jobId, message) as EventBatch;
  }

  // ---- Phase 5: .slxy save / load ----

  /** Builds .slxy archive bytes from the document + assets + host extra. */
  saveSlxy(extra: SaveExtra): Uint8Array {
    return this.app.save_slxy(extra) as Uint8Array;
  }

  /** Replaces the document from .slxy bytes; returns batch + view state. */
  loadSlxy(bytes: Uint8Array): SlxyLoadResult {
    return this.app.load_slxy(bytes) as SlxyLoadResult;
  }
}
