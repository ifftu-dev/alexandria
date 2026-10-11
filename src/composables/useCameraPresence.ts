// Camera presence for a standalone assessment attempt.
//
// Owns the getUserMedia stream, the off-screen <video> the backend reads
// frames from, and the two Sentinel loops (fast gaze, slower LBP identity /
// presence). The course player has its own copy of this flow; this one is
// the role-required variant: `enable()` may run before Sentinel starts
// (so the learner proves the camera works before the attempt exists), and
// `bind()` then opts the live session in once it is active.
//
// Nothing here persists imagery. Frames go to the backend in memory and
// only derived values come back (see docs/sentinel.md Privacy Guarantees).

import { onUnmounted, ref, type Ref } from 'vue'
import type { useSentinel } from './useSentinel'

const GAZE_LOOP_INTERVAL_MS = 1000
const FACE_ID_LOOP_INTERVAL_MS = 3000

type Sentinel = Pick<
  ReturnType<typeof useSentinel>,
  'isActive' | 'setCameraOptedIn' | 'reportCameraDevice' | 'verifyFace' | 'scoreGaze'
>

export function useCameraPresence(sentinel: Sentinel, videoRef: Ref<HTMLVideoElement | null>) {
  const stream = ref<MediaStream | null>(null)
  const starting = ref(false)
  const error = ref<string | null>(null)
  const lastFacePresent = ref<boolean | null>(null)
  /** Set when the OS or the learner ended the track outside our control. */
  const lost = ref(false)

  let generation = 0
  let gazeTimer: ReturnType<typeof setInterval> | null = null
  let faceTimer: ReturnType<typeof setInterval> | null = null
  let gazeInFlight = false
  let bound = false

  function stopLoops() {
    if (gazeTimer) clearInterval(gazeTimer)
    if (faceTimer) clearInterval(faceTimer)
    gazeTimer = faceTimer = null
    gazeInFlight = false
  }

  function startLoops() {
    if (gazeTimer || faceTimer) return
    const myGeneration = generation
    gazeTimer = setInterval(() => {
      const video = videoRef.value
      if (!video || gazeInFlight) return
      gazeInFlight = true
      void sentinel.scoreGaze(video).finally(() => {
        if (myGeneration === generation) gazeInFlight = false
      })
    }, GAZE_LOOP_INTERVAL_MS)
    faceTimer = setInterval(() => {
      const video = videoRef.value
      if (!video) return
      lastFacePresent.value = sentinel.verifyFace(video).present
    }, FACE_ID_LOOP_INTERVAL_MS)
  }

  function release() {
    if (stream.value) {
      for (const track of stream.value.getTracks()) track.stop()
      stream.value = null
    }
    if (videoRef.value) videoRef.value.srcObject = null
  }

  async function attach() {
    // Wait a tick so a v-if'd <video> exists before we hand it the stream.
    await new Promise(resolve => setTimeout(resolve, 0))
    const video = videoRef.value
    if (!video || !stream.value) return
    video.srcObject = stream.value
    video.muted = true
    try {
      await video.play()
    } catch {
      // Autoplay rules; the loops retry via readyState.
    }
  }

  /** Request the camera. Safe to call before Sentinel starts. */
  async function enable(): Promise<boolean> {
    if (stream.value || starting.value) return !!stream.value
    const myGeneration = ++generation
    const isCurrent = () => myGeneration === generation
    error.value = null
    lost.value = false
    starting.value = true
    try {
      const media = await navigator.mediaDevices.getUserMedia({
        video: { width: 320, height: 240, facingMode: 'user' },
        audio: false,
      })
      if (!isCurrent()) {
        for (const track of media.getTracks()) track.stop()
        return false
      }
      stream.value = media
      const track = media.getVideoTracks?.()[0]
      sentinel.reportCameraDevice(track?.label ?? null)
      track?.addEventListener('ended', () => {
        if (!isCurrent()) return
        lost.value = true
        disable()
      })
      await attach()
      if (!isCurrent()) return false
      if (bound && sentinel.isActive.value) {
        sentinel.setCameraOptedIn(true)
        startLoops()
      }
      return true
    } catch (e) {
      if (isCurrent()) {
        error.value = e instanceof Error ? e.message : String(e)
        release()
      }
      return false
    } finally {
      if (isCurrent()) starting.value = false
    }
  }

  /**
   * Opt the now-active Sentinel session in and start the loops. Called once
   * the attempt exists; a stream enabled earlier is picked up here.
   */
  function bind() {
    bound = true
    if (stream.value && sentinel.isActive.value) {
      sentinel.setCameraOptedIn(true)
      startLoops()
    }
  }

  function disable() {
    generation++
    starting.value = false
    stopLoops()
    release()
    lastFacePresent.value = null
    if (bound) sentinel.setCameraOptedIn(false)
  }

  function dispose() {
    // If the session outlives us (cleanup failed, stop retried later), its
    // snapshots must not keep crediting camera time that is no longer live.
    if (bound && sentinel.isActive.value) sentinel.setCameraOptedIn(false)
    bound = false
    generation++
    stopLoops()
    release()
    lastFacePresent.value = null
  }

  onUnmounted(dispose)

  return { stream, starting, error, lost, lastFacePresent, enable, bind, disable, dispose }
}
