// Virtual-camera detection from the MediaStreamTrack label.
//
// A webcam feed can be replaced by software (OBS Virtual Camera, ManyCam,
// Snap Camera…) that plays a recording or a deepfake into the browser's
// camera API. The browser exposes the device's human-readable name as the
// video track's `label`, which for these products carries the vendor name.
// This is a label heuristic, not liveness: a renamed device or a hardware
// capture card passes it. Liveness (MiniFASNet) is the stronger check and
// lands separately; this is the cheap first gate.
//
// Only the boolean verdict leaves the client. The label itself is kept in
// the dev debug view and never persisted.

/** Lower-cased substrings that identify virtual-camera products. */
export const VIRTUAL_CAMERA_MARKERS: readonly string[] = [
  'obs virtual camera',
  'obs-camera',
  'obs camera',
  'manycam',
  'snap camera',
  'camtwist',
  'mmhmm',
  'xsplit',
  'splitcam',
  'youcam',
  'chromacam',
  'iriun',
  'epoccam',
  'droidcam',
  'reincubate camo',
  'camo camera',
  'ndi video',
  'ndi virtual',
  'vcam',
  'virtual camera',
  'virtual cam',
  'virtualcam',
  'fake camera',
  'e2esoft',
  'altercam',
  'magic camera',
  'webcamoid',
  'v4l2loopback',
  'dummy video',
]

/**
 * True when a video track label names a known virtual-camera product.
 * Null / empty labels (permission not yet granted, or a privacy browser)
 * are "unknown", not virtual.
 */
export function isVirtualCameraLabel(label: string | null | undefined): boolean {
  if (!label) return false
  const l = label.toLowerCase()
  return VIRTUAL_CAMERA_MARKERS.some(marker => l.includes(marker))
}
