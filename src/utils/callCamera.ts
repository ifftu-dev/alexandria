// The webview's handle on the camera a call is publishing.
//
// On desktop the call captures the camera natively and the page shows a
// webview stream of it as the self view, because pushing frames over IPC is
// too slow for a smooth picture. Two separate captures must open the same
// device, or the self view shows one camera while everyone else sees
// another. The backend reports its camera by the system's name, which is
// also the label the webview gives that device.

/** The webview device id for the camera called `name`, if the webview has it. */
export async function cameraDeviceId(name: string | null | undefined): Promise<string | null> {
  if (!name || !navigator.mediaDevices?.enumerateDevices) return null
  const devices = await navigator.mediaDevices.enumerateDevices()
  return devices.find(device => device.kind === 'videoinput' && device.label === name)?.deviceId ?? null
}

/** Open the webview's own stream of the camera called `name`, or null. */
export async function openCameraPreview(name: string | null | undefined): Promise<MediaStream | null> {
  const deviceId = await cameraDeviceId(name)
  if (!deviceId) return null
  return navigator.mediaDevices.getUserMedia({
    video: {
      deviceId: { exact: deviceId },
      width: { ideal: 1280 },
      height: { ideal: 720 },
      frameRate: { ideal: 30 },
    },
    audio: false,
  })
}
