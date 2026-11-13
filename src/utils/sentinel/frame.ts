// A camera frame Sentinel can look at: the live <video> of a webview camera
// stream, or a decoded still. The still is how iOS calls are checked — there
// the call itself holds the camera and the webview only ever sees the JPEG
// self-preview frames the call hands it.
export type SentinelFrame = HTMLVideoElement | HTMLImageElement

/** The frame's pixel size, or null while it has nothing to draw yet. */
export function frameSize(frame: SentinelFrame): { width: number; height: number } | null {
  if (frame instanceof HTMLImageElement) {
    return frame.complete && frame.naturalWidth > 0
      ? { width: frame.naturalWidth, height: frame.naturalHeight }
      : null
  }
  return frame.readyState >= 2
    ? { width: frame.videoWidth || 640, height: frame.videoHeight || 480 }
    : null
}

// Frames cross IPC as a raw RGBA body (a Uint8Array over the canvas
// buffer), never as a JSON number array: about 150 KiB instead of half a
// megabyte of text per tick, with no per-pixel encode or parse. Size and
// identity travel in headers the backend reads beside the body
// (src-tauri/src/commands/sentinel_gaze.rs).

/** Longest side the webview sends; the backend detects at this size. */
export const FRAME_LONG_SIDE = 224

/** The draw size for a frame: longest side capped, aspect ratio kept. */
export function downscaledSize(
  size: { width: number; height: number },
  maxSide = FRAME_LONG_SIDE,
): { width: number; height: number } {
  const scale = Math.min(1, maxSide / Math.max(size.width, size.height))
  return {
    width: Math.max(1, Math.round(size.width * scale)),
    height: Math.max(1, Math.round(size.height * scale)),
  }
}

/** The canvas pixels as an IPC body, sharing the canvas buffer. */
export function rgbaBody(image: ImageData): Uint8Array {
  return new Uint8Array(image.data.buffer, image.data.byteOffset, image.data.byteLength)
}

export interface FrameIdentity {
  user: string
  device: string
  /** A standalone preview that must not supply assessment evidence. */
  preview?: boolean
}

/** Headers that describe a raw frame body. */
export function frameHeaders(
  width: number,
  height: number,
  identity?: FrameIdentity,
): Record<string, string> {
  const headers: Record<string, string> = {
    'x-sentinel-frame-width': String(width),
    'x-sentinel-frame-height': String(height),
  }
  if (identity) {
    headers['x-sentinel-user'] = identity.user
    headers['x-sentinel-device'] = identity.device
    if (identity.preview) headers['x-sentinel-preview'] = '1'
  }
  return headers
}
