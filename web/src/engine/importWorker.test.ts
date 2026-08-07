// The import worker's message contract for hierarchy builds.
//
// Mock-heavy where the rest of this suite is not, and it earns that: the
// branch under test is glue, and the one thing glue gets silently wrong is the
// transfer list. A build's two buffers are the largest things that cross this
// boundary, and omitting them from the list copies rather than moves, which
// nothing fails on and everything slows down.

import { beforeEach, describe, expect, it, vi } from "vitest";

const buildBvhJob = vi.fn(
  (_positions: Float32Array, _indices: Uint32Array) => new Uint8Array([1, 2, 3, 4]),
);

vi.mock("../wasm/pkg/solarxy_web.js", () => ({
  default: vi.fn(() => Promise.resolve()),
  build_bvh_job: (positions: Float32Array, indices: Uint32Array) =>
    buildBvhJob(positions, indices),
  parse_model_job: vi.fn(),
  prepare_hdri_job: vi.fn(),
  validate_geometry_job: vi.fn(),
}));
vi.mock("../wasm/pkg/solarxy_web_bg.wasm?url", () => ({ default: "solarxy_web_bg.wasm" }));
vi.mock("./draco", () => ({ maybeInflateDraco: vi.fn() }));

interface Posted {
  message: Record<string, unknown>;
  transfer?: Transferable[];
}

const posted: Posted[] = [];

/** Drives the worker's handler and returns what it posted back. */
async function post(message: Record<string, unknown>): Promise<Posted> {
  const handler = (globalThis as unknown as { onmessage: (e: unknown) => Promise<void> })
    .onmessage;
  await handler({ data: message });
  return posted[posted.length - 1];
}

beforeEach(async () => {
  posted.length = 0;
  buildBvhJob.mockClear();
  Object.assign(globalThis, {
    self: globalThis,
    postMessage: (message: Record<string, unknown>, transfer?: Transferable[]) => {
      posted.push({ message, transfer });
    },
  });
  vi.resetModules();
  await import("./importWorker");
});

describe("the buildBvh worker branch", () => {
  it("answers with the packed hierarchy under its own job id", async () => {
    const positions = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    const indices = new Uint32Array([0, 1, 2]);
    const { message } = await post({
      kind: "buildBvh",
      jobId: -2_000_000,
      ctx: "root",
      positions,
      indices,
    });

    expect(buildBvhJob).toHaveBeenCalledWith(positions, indices);
    expect(message.kind).toBe("buildBvh");
    expect(message.jobId).toBe(-2_000_000);
    expect(message.bvh).toEqual(new Uint8Array([1, 2, 3, 4]));
  });

  it("transfers the result rather than copying it", async () => {
    const { message, transfer } = await post({
      kind: "buildBvh",
      jobId: -1,
      ctx: "root",
      positions: new Float32Array([0, 0, 0]),
      indices: new Uint32Array([0]),
    });
    expect(transfer).toEqual([(message.bvh as Uint8Array).buffer]);
  });

  it("reports a build that threw as a non-fatal error on the same kind", async () => {
    // A build cannot fail on input the way a parse can, so anything that
    // escapes it is our bug. What matters here is that the reply still carries
    // the kind and the id, or the waiting promise never settles.
    buildBvhJob.mockImplementationOnce(() => {
      throw new Error("out of memory");
    });
    const { message } = await post({
      kind: "buildBvh",
      jobId: -7,
      ctx: "root",
      positions: new Float32Array([0, 0, 0]),
      indices: new Uint32Array([0]),
    });
    expect(message.kind).toBe("buildBvh");
    expect(message.jobId).toBe(-7);
    expect(message.error).toBe("out of memory");
    expect(message.fatal).toBe(false);
    expect(message.bvh).toBeUndefined();
  });
});
