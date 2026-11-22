import {
  MAX_MS,
  SAMPLE_MS,
  EDGES,
  visible,
  compare,
  trackable,
  validFrames,
  poseAt,
  armRaise,
} from './motion.js'

const el = (id) => document.getElementById(id)
const camera = el('camera'),
  clip = el('clip')
const start = el('start'),
  record = el('record'),
  stop = el('stop'),
  finish = el('finish')
const status = (message) => {
  el('status').textContent = message
}
const MAX_CLIP_BYTES = 1200000
let author = false,
  frames = [],
  duration = 0,
  clipUrl = null
let stream = null,
  detector = null,
  session = 0,
  recording = null,
  busy = false
let previewStart = null,
  playFrame = 0,
  hadComparison = false,
  aligned = false

function draw(id, pose, color) {
  const canvas = el(id),
    ctx = canvas.getContext('2d')
  ctx.clearRect(0, 0, canvas.width, canvas.height)
  if (!pose) return
  ctx.strokeStyle = ctx.fillStyle = color
  ctx.lineWidth = 4
  ctx.lineCap = 'round'
  for (const [a, b] of EDGES) {
    if (!visible(pose[a]) || !visible(pose[b])) continue
    ctx.beginPath()
    ctx.moveTo(pose[a][0] * 0.4, pose[a][1] * 0.4)
    ctx.lineTo(pose[b][0] * 0.4, pose[b][1] * 0.4)
    ctx.stroke()
  }
  for (const [x, y, score] of pose.slice(5)) {
    if (score < 35) continue
    ctx.beginPath()
    ctx.arc(x * 0.4, y * 0.4, 3.5, 0, Math.PI * 2)
    ctx.fill()
  }
}

function haltPreview() {
  cancelAnimationFrame(playFrame)
  previewStart = null
  clip.pause()
}
function phaseTime() {
  if (!clip.hidden && !clip.paused) return clip.currentTime * 1000
  return (performance.now() - previewStart) % duration
}
function playReference(loop) {
  haltPreview()
  previewStart = performance.now()
  if (!clip.hidden) {
    clip.currentTime = 0
    clip.loop = loop
    clip.play().catch(() => {
      clip.hidden = true
    })
  }
  function paint() {
    if (previewStart === null) return
    if (!loop && performance.now() - previewStart >= duration) {
      haltPreview()
      return
    }
    draw('target', poseAt(frames, phaseTime()), '#fac078')
    playFrame = requestAnimationFrame(paint)
  }
  paint()
}
function loadReference(content) {
  haltPreview()
  clip.removeAttribute('src')
  clip.hidden = true
  if (clipUrl) URL.revokeObjectURL(clipUrl)
  clipUrl = null
  frames =
    content?.preset === 'arm-raise'
      ? armRaise()
      : validFrames(content?.frames)
        ? content.frames
        : []
  const lastTime = frames.at(-1)?.[0] ?? 0
  duration = Math.min(
    MAX_MS,
    Math.max(
      lastTime,
      Number.isFinite(content?.duration) ? content.duration : 0,
    ),
  )
  const video = content?.clip
  if (
    frames.length &&
    ['video/mp4', 'video/webm'].includes(video?.type) &&
    clip.canPlayType(video.type) &&
    typeof video.data === 'string' &&
    video.data.length <= Math.ceil(MAX_CLIP_BYTES / 3) * 4
  ) {
    try {
      const bytes = Uint8Array.from(atob(video.data), (c) => c.charCodeAt(0))
      clipUrl = URL.createObjectURL(new Blob([bytes], { type: video.type }))
      clip.src = clipUrl
      clip.hidden = false
    } catch {
      /* The recorded skeleton still plays when a codec is unavailable. */
    }
  }
  el('target-label').textContent =
    content?.preset === 'arm-raise'
      ? 'Example · standing arm raise'
      : 'Coach’s demonstration'
  el('preview').hidden = !frames.length
  draw('target', frames[0]?.[1], '#fac078')
}
clip.onerror = () => {
  clip.hidden = true
}

function resetCamera() {
  session++
  busy = false
  if (recording) {
    clearTimeout(recording.timer)
    if (recording.recorder?.state === 'recording') recording.recorder.stop()
    recording = null
  }
  stream?.getTracks().forEach((track) => track.stop())
  stream = null
  camera.srcObject = null
  detector?.close()
  detector = null
  aligned = false
  start.disabled = !author && !frames.length
  stop.hidden = record.hidden = true
  el('preview').disabled = false
  start.textContent = 'Start camera'
  haltPreview()
  draw('live', null, '')
  el('feedback').textContent = ''
}
async function sample(id) {
  if (id !== session) return
  try {
    if (camera.readyState >= 2) {
      const reference =
        !author && previewStart !== null ? poseAt(frames, phaseTime()) : null
      const sampleTime = performance.now()
      const pose = await detector.detect(camera)
      if (id !== session) return
      aligned = trackable(pose)
      draw('live', pose, '#65ead3')
      if (recording && aligned && sampleTime >= recording.start) {
        const ms = Math.round(sampleTime - recording.start)
        if (ms <= MAX_MS) recording.frames.push([ms, pose])
      }
      if (author) {
        record.disabled = !recording && !aligned
        el('feedback').textContent = aligned
          ? recording
            ? 'Recording movement…'
            : 'Pose in view'
          : 'Step back so your joints are visible.'
      } else {
        const joints = aligned && reference ? compare(pose, reference) : []
        if (joints.length >= 3) {
          const joint = joints[0]
          if (joint.delta >= 15) {
            const [x, y] = pose[joint.index]
            const ctx = el('live').getContext('2d')
            ctx.strokeStyle = '#fac078'
            ctx.beginPath()
            ctx.arc(x * 0.4, y * 0.4, 10, 0, 2 * Math.PI)
            ctx.stroke()
          }
          el('feedback').textContent =
            joint.delta < 15
              ? 'Your visible joint angles match the demonstration.'
              : joint.name +
                ' · you ' +
                Math.round(joint.actual) +
                '° · coach ' +
                Math.round(joint.expected) +
                '°'
          hadComparison = true
          finish.hidden = false
        } else {
          el('feedback').textContent = reference
            ? 'Step back and face the same direction as the coach.'
            : 'Waiting for the next tracked reference pose.'
        }
      }
    }
    setTimeout(() => sample(id), SAMPLE_MS)
  } catch (err) {
    if (id === session) {
      resetCamera()
      status('Tracking stopped. ' + err.message)
    }
  }
}

start.onclick = async () => {
  const id = ++session
  start.disabled = true
  stop.hidden = false
  try {
    const inference = await window.alex.requestCapability(
      'ml_inference',
      'Use MoveNet to compare visible joint angles.',
    )
    if (id !== session) return
    if (!inference.granted)
      throw Error(
        'Pose tracking requires permission to run MoveNet on this device.',
      )
    const permission = await window.alex.requestCapability(
      'camera',
      'Track your movement on this device.',
    )
    if (id !== session) return
    if (!permission.granted)
      throw Error('Camera permission is required to track movement.')
    status('Loading MoveNet on this device…')
    const { openDetector } = await import('./vendor/movenet.js')
    if (id !== session) return
    const opened = await openDetector()
    if (id !== session) {
      opened.close()
      return
    }
    detector = opened
    const media = await navigator.mediaDevices.getUserMedia({
      video: {
        facingMode: 'user',
        width: { ideal: 640 },
        height: { ideal: 480 },
      },
      audio: false,
    })
    if (id !== session) {
      media.getTracks().forEach((t) => t.stop())
      return
    }
    stream = media
    media.getVideoTracks()[0].onended = () => {
      if (id === session) {
        resetCamera()
        status('Camera disconnected.')
      }
    }
    camera.srcObject = media
    await camera.play()
    if (id !== session) return
    record.hidden = !author
    record.disabled = true
    record.textContent = 'Record movement'
    el('preview').disabled = true
    status(
      author
        ? 'Frame your movement, then record up to 12 seconds.'
        : 'Follow the coach at the same pace and camera angle.',
    )
    if (!author) playReference(true)
    void sample(id)
  } catch (err) {
    if (id === session) {
      resetCamera()
      status(err.message || 'Camera unavailable.')
    }
  }
}
function encodeClip(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () =>
      resolve({
        type: blob.type.split(';')[0],
        data: reader.result.split(',')[1],
      })
    reader.onerror = () => reject(Error('Could not read the recording.'))
    reader.readAsDataURL(blob)
  })
}
function beginRecording() {
  let recorder = null
  const chunks = []
  if (typeof MediaRecorder !== 'undefined') {
    const mimeType = ['video/webm', 'video/mp4'].find((type) =>
      MediaRecorder.isTypeSupported(type),
    )
    if (mimeType)
      recorder = new MediaRecorder(stream, {
        mimeType,
        videoBitsPerSecond: 400000,
      })
  }
  const video =
    recorder &&
    new Promise((resolve) => {
      recorder.ondataavailable = (e) => {
        if (e.data.size) chunks.push(e.data)
      }
      recorder.onstop = () =>
        resolve(new Blob(chunks, { type: recorder.mimeType }))
      recorder.onerror = () => resolve(null)
    })
  recorder?.start()
  recording = {
    recorder,
    video,
    frames: [],
    start: performance.now(),
    timer: setTimeout(endRecording, MAX_MS),
  }
  record.textContent = 'Use this take'
  status(
    recorder
      ? 'Recording the video and pose reference.'
      : 'Recording a pose reference on this device.',
  )
}
async function endRecording() {
  const take = recording
  if (!take || busy) return
  busy = true
  recording = null
  clearTimeout(take.timer)
  const id = session
  const length = Math.min(MAX_MS, Math.round(performance.now() - take.start))
  try {
    if (take.recorder?.state === 'recording') take.recorder.stop()
    if (!validFrames(take.frames))
      throw Error('Record a longer take with your body in view.')
    const content = { frames: take.frames, duration: length }
    const video = await take.video
    if (video?.size && video.size <= MAX_CLIP_BYTES)
      content.clip = await encodeClip(video)
    if (id !== session) return
    await window.alex.emitEvent('author_config', content)
    if (id !== session) return
    loadReference(content)
    resetCamera()
    status(
      content.clip
        ? 'Take ready. Replay it, then save the lesson.'
        : 'Pose replay ready. Save the lesson to keep this take.',
    )
  } catch (err) {
    if (id === session) {
      resetCamera()
      status(err.message)
    }
  }
}
record.onclick = () => {
  if (busy) return
  if (recording) {
    void endRecording()
    return
  }
  try {
    beginRecording()
  } catch (err) {
    resetCamera()
    status(err.message)
  }
}
stop.onclick = () => {
  resetCamera()
  status('Camera stopped.')
}
finish.onclick = async () => {
  if (!hadComparison) return
  resetCamera()
  await window.alex.complete(1)
  finish.hidden = true
  status('Practice complete.')
}
el('preview').onclick = () => playReference(false)
function leave() {
  resetCamera()
  status('Camera stopped.')
}
window.addEventListener('pagehide', leave)
document.addEventListener('visibilitychange', () => {
  if (document.hidden) leave()
})
window.alex.onHost(({ type, payload }) => {
  if (
    type === 'capability_revoked' &&
    ['camera', 'ml_inference'].includes(payload.name)
  )
    leave()
  if (type !== 'init') return
  author = payload.mode === 'author'
  loadReference(payload.content)
  start.disabled = !author && !frames.length
  el('instruction').textContent = author
    ? 'Record one repetition from an angle your learners can reproduce.'
    : 'Watch the demonstration, then practise in front of your camera.'
  status(
    author
      ? 'Your video and tracked joints will be included in the lesson.'
      : frames.length
        ? 'Your camera stays on this device.'
        : 'The coach has not added a demonstration yet.',
  )
  if (!payload.granted_capabilities.includes('camera'))
    start.textContent = 'Enable camera'
  else start.textContent = 'Start camera'
})
window.alex.ready(['camera', 'ml_inference'])
