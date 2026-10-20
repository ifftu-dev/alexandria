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
