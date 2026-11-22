export const MAX_MS = 12000
export const SAMPLE_MS = 125
export const EDGES = [
  [5, 6],
  [5, 7],
  [7, 9],
  [6, 8],
  [8, 10],
  [5, 11],
  [6, 12],
  [11, 12],
  [11, 13],
  [13, 15],
  [12, 14],
  [14, 16],
]
const JOINTS = [
  ['Left elbow', 5, 7, 9],
  ['Right elbow', 6, 8, 10],
  ['Left shoulder', 7, 5, 11],
  ['Right shoulder', 8, 6, 12],
  ['Left hip', 5, 11, 13],
  ['Right hip', 6, 12, 14],
  ['Left knee', 11, 13, 15],
  ['Right knee', 12, 14, 16],
]
export const visible = (p) => p[2] >= 35
function angle(pose, a, b, c) {
  if (![a, b, c].every((i) => visible(pose[i]))) return null
  const dx = (i) => pose[i][0] - pose[b][0]
  const dy = (i) => pose[i][1] - pose[b][1]
  const length = Math.hypot(dx(a), dy(a)) * Math.hypot(dx(c), dy(c))
  if (!length) return null
  const cosine = (dx(a) * dx(c) + dy(a) * dy(c)) / length
  return Math.acos(Math.max(-1, Math.min(1, cosine))) * 180 / Math.PI
}
export function compare(live, target) {
  const results = []
  for (const [name, a, b, c] of JOINTS) {
    const actual = angle(live, a, b, c),
      expected = angle(target, a, b, c)
    if (actual !== null && expected !== null)
      results.push({
        name,
        index: b,
        actual,
        expected,
        delta: Math.abs(actual - expected),
      })
  }
  return results.sort((a, b) => b.delta - a.delta)
}
export const trackable = (pose) =>
  JOINTS.filter(([, a, b, c]) => angle(pose, a, b, c) !== null).length >= 3

export function validFrames(frames) {
  return (
    Array.isArray(frames) &&
    frames.length >= 4 &&
    frames.length <= 100 &&
    frames.every((frame, i) => {
      return (
        Array.isArray(frame) &&
        frame.length === 2 &&
        Number.isInteger(frame[0]) &&
        frame[0] >= 0 &&
        frame[0] <= MAX_MS &&
        (!i || frame[0] > frames[i - 1][0]) &&
        Array.isArray(frame[1]) &&
        frame[1].length === 17 &&
        frame[1].every(
          (p) =>
            Array.isArray(p) &&
            p.length === 3 &&
            p.every(Number.isFinite) &&
            p[2] >= 0 &&
            p[2] <= 100 &&
            Math.abs(p[0]) <= 2000 &&
            Math.abs(p[1]) <= 2000,
        )
      )
    })
  )
}

// Do not compare an old pose during a gap in the coach's tracking.
export function poseAt(frames, time) {
  let nearest = frames[0]
  for (const frame of frames) {
    if (Math.abs(frame[0] - time) > Math.abs(nearest[0] - time)) break
    nearest = frame
  }
  return Math.abs(nearest[0] - time) <= 400 ? nearest[1] : null
}

export function armRaise() {
  const frames = []
  for (let ms = 0; ms <= 4000; ms += SAMPLE_MS) {
    const pose = Array.from({ length: 17 }, () => [500, 220, 0])
    const angle = ((1 - Math.cos((ms / 4000) * 2 * Math.PI)) * Math.PI) / 4
    for (const [side, sign] of [
      [0, 1],
      [1, -1],
    ]) {
      const shoulder = 500 + sign * 70
      pose[5 + side] = [shoulder, 300, 100]
      for (const [i, len] of [
        [7, 105],
        [9, 210],
      ]) {
        pose[i + side] = [
          Math.round(shoulder + sign * len * Math.sin(angle)),
          Math.round(300 + len * Math.cos(angle)),
          100,
        ]
      }
      pose[11 + side] = [500 + sign * 55, 530, 100]
      pose[13 + side] = [500 + sign * 60, 690, 100]
      pose[15 + side] = [500 + sign * 68, 850, 100]
    }
    frames.push([ms, pose])
  }
  return frames
}
