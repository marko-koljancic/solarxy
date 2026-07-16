// Turntable export encoders (item 9b). All three formats are built from the
// same deterministic offscreen frames the host renders through the camera:
// a PNG sequence zipped with fflate, and WebM / MP4 via WebCodecs + the
// matching muxer. WebCodecs is Chrome-available (the standing verification
// target); PNG-ZIP is the pure-JS fallback when video encoding is absent.

import { zipSync } from "fflate";
import { Muxer as Mp4Muxer, ArrayBufferTarget as Mp4Target } from "mp4-muxer";
import { Muxer as WebmMuxer, ArrayBufferTarget as WebmTarget } from "webm-muxer";
import type { ScreenshotResult } from "../engine/types";

export type TurntableFormat = "webm" | "mp4" | "pngZip";

/** WebM/MP4 need WebCodecs; PNG-ZIP does not. */
export function videoExportSupported(): boolean {
  return typeof VideoEncoder !== "undefined" && typeof VideoFrame !== "undefined";
}

export function formatExtension(format: TurntableFormat): string {
  return format === "pngZip" ? "zip" : format;
}

async function frameToPng(f: ScreenshotResult): Promise<Uint8Array> {
  const canvas = new OffscreenCanvas(f.width, f.height);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2d context unavailable");
  // Copy into a fresh clamped array (the wasm Uint8Array types as
  // ArrayBufferLike, which ImageData rejects).
  const clamped = new Uint8ClampedArray(f.pixels.length);
  clamped.set(f.pixels);
  ctx.putImageData(new ImageData(clamped, f.width, f.height), 0, 0);
  const blob = await canvas.convertToBlob({ type: "image/png" });
  return new Uint8Array(await blob.arrayBuffer());
}

export async function framesToPngZip(frames: ScreenshotResult[]): Promise<Blob> {
  const files: Record<string, Uint8Array> = {};
  for (let i = 0; i < frames.length; i++) {
    files[`frame_${String(i).padStart(4, "0")}.png`] = await frameToPng(frames[i]);
  }
  // level 0: the PNGs are already compressed, so store rather than re-deflate.
  const zipped = zipSync(files, { level: 0 });
  return new Blob([zipped as BlobPart], { type: "application/zip" });
}

export async function framesToVideo(
  frames: ScreenshotResult[],
  fps: number,
  container: "mp4" | "webm",
): Promise<Blob> {
  if (!videoExportSupported()) {
    throw new Error("Video export needs WebCodecs, which this browser does not provide");
  }
  if (frames.length === 0) throw new Error("no frames to encode");
  const { width, height } = frames[0];
  const mp4Target = new Mp4Target();
  const webmTarget = new WebmTarget();
  const muxer =
    container === "mp4"
      ? new Mp4Muxer({
          target: mp4Target,
          video: { codec: "avc", width, height, frameRate: fps },
          fastStart: "in-memory",
        })
      : new WebmMuxer({
          target: webmTarget,
          video: { codec: "V_VP9", width, height, frameRate: fps },
        });

  let encoderError: unknown = null;
  const encoder = new VideoEncoder({
    output: (chunk, meta) =>
      (muxer as { addVideoChunk: (c: EncodedVideoChunk, m?: EncodedVideoChunkMetadata) => void })
        .addVideoChunk(chunk, meta),
    error: (e) => {
      encoderError = e;
    },
  });
  encoder.configure({
    codec: container === "mp4" ? "avc1.42001f" : "vp09.00.10.08",
    width,
    height,
    framerate: fps,
    bitrate: Math.min(24_000_000, Math.round(width * height * fps * 0.2)),
  });

  const frameDurationUs = Math.round(1_000_000 / fps);
  for (let i = 0; i < frames.length; i++) {
    const f = frames[i];
    const buf = new Uint8Array(f.pixels.length);
    buf.set(f.pixels);
    const vf = new VideoFrame(buf, {
      format: "RGBA",
      codedWidth: f.width,
      codedHeight: f.height,
      timestamp: i * frameDurationUs,
      duration: frameDurationUs,
    });
    // A periodic keyframe keeps seeking usable without bloating the file.
    encoder.encode(vf, { keyFrame: i % 30 === 0 });
    vf.close();
    if (encoderError) throw encoderError;
  }
  await encoder.flush();
  encoder.close();
  if (encoderError) throw encoderError;
  muxer.finalize();

  const buffer = container === "mp4" ? mp4Target.buffer : webmTarget.buffer;
  return new Blob([buffer as BlobPart], {
    type: container === "mp4" ? "video/mp4" : "video/webm",
  });
}

/** Encodes the captured frames to the chosen format. */
export function encodeTurntable(
  frames: ScreenshotResult[],
  fps: number,
  format: TurntableFormat,
): Promise<Blob> {
  if (format === "pngZip") return framesToPngZip(frames);
  return framesToVideo(frames, fps, format);
}
