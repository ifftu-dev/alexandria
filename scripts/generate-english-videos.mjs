#!/usr/bin/env node
// Rebuild the narrated lesson videos for "English for Hindi Speakers".
//
// Reads demo-world/content/english-videos.json and writes one MP4 per video
// to demo-world/content/videos/, then records each duration back into the
// JSON. Each script step is one speech segment: Hindi narration uses the
// macOS Lekha voice, English examples a US voice, and the second speaker in
// dialogues an Indian English voice. Slides are rendered in Chromium so
// Devanagari is shaped correctly, and each step's slide reveals that step's
// line. Requires macOS `say`, FFmpeg, and the Playwright Chromium build
// (`npx playwright install chromium`).
//
// Usage: node scripts/generate-english-videos.mjs [video-id ...]

import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { chromium } from 'playwright-core'

const ROOT = resolve(import.meta.dirname, '..')
const CONTENT = join(ROOT, 'demo-world/content')
const SPEC = join(CONTENT, 'english-videos.json')
const VOICES = {
  hi: { name: 'Lekha', rate: 165, label: 'हिंदी' },
  en: { name: 'Samantha', rate: 145, label: 'English' },
  en2: { name: 'Rishi', rate: 150, label: 'English' },
}
const GAP = 0.35 // seconds of silence after every step
const RATE = 22050

// macOS speech synthesis occasionally stalls; time out and retry once.
function run(cmd, args) {
  const options = { stdio: ['ignore', 'ignore', 'inherit'], timeout: 120_000, killSignal: 'SIGKILL' }
  try {
    execFileSync(cmd, args, options)
  } catch (error) {
    if (error.code !== 'ETIMEDOUT') throw error
    console.warn(`${cmd} timed out; retrying`)
    execFileSync(cmd, args, options)
  }
}
const duration = file =>
  Number(execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', file]).toString())
const esc = text =>
  String(text ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

// Short lines (word lists, pairs) fit two columns of three. Longer lines
// stack in one column, and only the latest four stay on screen.
const SHORT_SHOW = 24
const SHORT_SUB = 30
const STACK_LIMIT = 4

function slide({ video, chapter, scene, sceneIndex, sceneCount, shown, current, voice, repeat = false }) {
  const short = shown.every(i => i.show.length <= SHORT_SHOW && (i.sub ?? '').length <= SHORT_SUB)
  const columns = short && shown.length > 3
  const offset = short ? 0 : Math.max(0, shown.length - STACK_LIMIT)
  const items = shown
    .slice(offset)
    .map((item, n) => {
      const i = n + offset
      return `<li class="${i === current ? 'now' : i < current || current < 0 ? 'past' : ''}">
        <div class="show">${esc(item.show)}</div>${item.sub ? `<div class="sub">${esc(item.sub)}</div>` : ''}</li>`
    })
    .join('')
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  * { box-sizing: border-box; margin: 0; padding: 0 }
  body { width: 1280px; height: 720px; overflow: hidden; color: #fff; padding: 40px 64px 30px;
    display: flex; flex-direction: column;
    background: radial-gradient(circle at 92% 8%, #f59e0b33 0, transparent 38%), linear-gradient(135deg, #1e1b4b, #312e81);
    font-family: 'Avenir Next', 'Kohinoor Devanagari', system-ui, sans-serif; }
  header { display: flex; justify-content: space-between; align-items: center; min-height: 40px;
    font-size: 20px; letter-spacing: .04em; color: #c7d2fe }
  .pill { padding: 6px 16px; border-radius: 999px; font-size: 20px; font-weight: 600 }
  .pill.hi { background: #f59e0b; color: #1e1b4b } .pill.en { background: #e0e7ff; color: #1e1b4b }
  .pill.turn { background: #34d399; color: #064e3b }
  .bar { margin-top: 10px; width: 96px; height: 6px; flex: none; background: #f59e0b; border-radius: 3px }
  h1 { margin-top: 22px; font-size: 44px; line-height: 1.25; font-weight: 700 }
  ul { margin-top: 30px; flex: 1; min-height: 0; overflow: hidden; list-style: none; display: grid; align-content: start;
    gap: 16px 40px; ${columns ? 'grid-template-columns: 1fr 1fr; grid-template-rows: repeat(3, auto); grid-auto-flow: column' : 'grid-template-columns: 1fr'} }
  li { border-left: 6px solid transparent; padding: 4px 0 4px 20px; opacity: .55 }
  li.now { border-color: #f59e0b; opacity: 1 } li.past { opacity: .7 }
  .show { font-size: ${shown.length > 3 ? 36 : 40}px; font-weight: 600; line-height: 1.28 }
  .sub { font-size: 25px; color: #c7d2fe; margin-top: 2px; line-height: 1.35 }
  footer { display: flex; justify-content: space-between; padding-top: 12px; font-size: 17px; color: #a5b4fc }
  </style></head><body>
  <header><span>ENGLISH FOR HINDI SPEAKERS · ${esc(chapter)}</span>${repeat ? '<span class="pill turn">🎙 अब आप बोलिए · Your turn</span>' : `<span class="pill ${voice === 'hi' ? 'hi' : 'en'}">🔊 ${VOICES[voice].label}</span>`}</header>
  <div class="bar"></div>
  <h1>${esc(scene.title)}</h1>
  <ul>${items}</ul>
  <footer><span>${esc(video.title)}</span><span>AI-generated · synthetic narration · ${sceneIndex + 1} / ${sceneCount}</span></footer>
  </body></html>`
}

async function render(page, html, file) {
  await page.setContent(html)
  await page.evaluate(() => document.fonts.ready)
  await page.screenshot({ path: file })
}

function speak(step, base) {
  const voice = VOICES[step.voice]
  if (!voice) throw new Error(`unknown voice ${step.voice}`)
  const text = `${base}.txt`
  const aiff = `${base}.aiff`
  const wav = `${base}.wav`
  writeFileSync(text, step.say)
  run('say', ['-v', voice.name, '-r', String(voice.rate), '-f', text, '-o', aiff])
  const pad = GAP + (step.pause ?? 0)
  const speech = duration(aiff)
  run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', aiff, '-af', `apad=pad_dur=${pad}`, '-ar', String(RATE), '-ac', '1', wav])
  return { wav, seconds: duration(wav), speech }
}

async function build(page, video, work) {
  const chapter = `CHAPTER ${video.lesson_element_id.split('_')[2]}`
  const images = []
  const audio = []
  for (const [sceneIndex, scene] of video.scenes.entries()) {
    const shown = []
    for (const [stepIndex, step] of scene.steps.entries()) {
      const base = join(work, `${video.id}-${sceneIndex}-${stepIndex}`)
      if (step.show) shown.push({ show: step.show, sub: step.sub })
      const current = step.show ? shown.length - 1 : -1
      const png = `${base}.png`
      const state = { video, chapter, scene, sceneIndex, sceneCount: video.scenes.length, shown, current, voice: step.voice }
      await render(page, slide(state), png)
      const { wav, seconds, speech } = speak(step, base)
      audio.push(wav)
      if (step.pause) {
        // Hold the line on screen while it is spoken, then cue the learner.
        const turn = `${base}-turn.png`
        await render(page, slide({ ...state, repeat: true }), turn)
        images.push({ png, seconds: speech + GAP }, { png: turn, seconds: seconds - speech - GAP })
      } else {
        images.push({ png, seconds })
      }
    }
  }
  const imageList = join(work, `${video.id}-images.txt`)
  // The concat demuxer ignores the last entry's duration unless it repeats.
  writeFileSync(
    imageList,
    images.map(i => `file '${i.png}'\nduration ${i.seconds.toFixed(3)}\n`).join('') + `file '${images.at(-1).png}'\n`,
  )
  const audioList = join(work, `${video.id}-audio.txt`)
  writeFileSync(audioList, audio.map(a => `file '${a}'\n`).join(''))
  const output = join(CONTENT, 'videos', `${video.id}.mp4`)
  run('ffmpeg', [
    '-hide_banner', '-loglevel', 'error', '-y',
    '-f', 'concat', '-safe', '0', '-i', imageList,
    '-f', 'concat', '-safe', '0', '-i', audioList,
    '-vf', 'fps=12,format=yuv420p', '-c:v', 'libx264', '-preset', 'veryfast', '-tune', 'stillimage', '-crf', '26',
    '-c:a', 'aac', '-b:a', '48k', '-ac', '1', '-ar', String(RATE),
    '-shortest', '-movflags', '+faststart', output,
  ])
  return Math.round(duration(output))
}

const spec = JSON.parse(readFileSync(SPEC, 'utf8'))
const only = new Set(process.argv.slice(2))
const work = mkdtempSync(join(tmpdir(), 'alexandria-english-video-'))
const browser = await chromium.launch()
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } })
  for (const video of spec.videos) {
    if (only.size && !only.has(video.id)) continue
    video.duration_seconds = await build(page, video, work)
    console.log(video.id, video.duration_seconds)
    // Save after each video so an interrupted run keeps finished durations.
    writeFileSync(SPEC, JSON.stringify(spec, null, 2) + '\n')
  }
} finally {
  await browser.close()
  rmSync(work, { recursive: true, force: true })
}
