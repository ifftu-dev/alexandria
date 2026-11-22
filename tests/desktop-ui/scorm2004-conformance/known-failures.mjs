// Rewrites `adl-known-failures.json` beside this file: the test suite cases
// the player does not pass yet, each with its matrix row, why, and every
// failure, which `scorm2004-conformance.spec.ts` pins. Record a run in both
// engines, then classify any new failing case in ROW below:
//
//   ADL_RECORD=/tmp/adl.jsonl npx playwright test scorm2004-conformance -g "ADL LMS"
//   node tests/desktop-ui/scorm2004-conformance/known-failures.mjs /tmp/adl.jsonl
//
// A case must fail the same way in every engine and run.
import fs from 'node:fs'

const recFile = process.argv[2]
const outFile = new URL('./adl-known-failures.json', import.meta.url)
if (!recFile) throw new Error('usage: known-failures.mjs <recorded run>')
const WHY = {
  choice: 'The choices offered differ from the suite\'s: the runtime\'s choice validity (constrainChoice applied to the wrong activity, stopForwardTraversal not stopping a choice, choiceExit and forwardOnly edge cases), and the table of contents has no entry for the root of the activity tree.',
  delivery: 'The sequencing engine delivers a different activity from the one the suite\'s pseudo-code run expects.',
  nothing: 'The sequencing engine delivers nothing where the suite expects an activity.',
  end: 'An exit rule or exit-parent rule ends the attempt on the root, but the engine does not end the sequencing session; the player offers a choice instead.',
  objectives: 'Objective data a lesson should read through global objective maps (completion, progress measure, raw, min and max scores) does not reach cmi.objectives.',
  validation: 'The runtime accepts values the standard refuses (timestamps with a zone after minutes, more than two fraction digits or years past 2038; durations without T; identifiers such as "urn:"; unknown language tags; malformed response patterns), answers 403 or 0 instead of 301 for a read past the end of a list, and 408 where the suite expects 351.',
  packages: 'The case continues in a second package and expects data shared across packages (adlcp:sharedDataGlobalToSystem); each package keeps its own data by design.',
}
const ROW = {
  choice: ['CM-07a', 'CM-07b', 'CM-07c', 'CM-07d', 'CM-07e', 'CM-07f', 'CM-16', 'CM-17a', 'CM-17b', 'SX-04b'],
  delivery: ['CM-04b', 'CM-06', 'CM-09aa', 'CM-09ba', 'CM-09ca', 'CO-02a', 'CO-03', 'CO-04a', 'CO-04b', 'CO-04c', 'CO-05a', 'CO-05b', 'CO-07b', 'CO-12d', 'CO-13a', 'MS-05a', 'MS-06', 'OB-03c', 'OB-09b', 'RU-10', 'RU-11', 'RU-13d', 'RU-15b', 'RU-19b'],
  nothing: ['CM-03b', 'CM-04c', 'RU-07c', 'T-01a', 'T-01b'],
  end: ['CO-07a', 'OB-08a', 'OB-08b', 'OB-09a'],
  objectives: ['CO-10', 'SX-11c'],
  validation: ['DMI'],
  packages: ['DDM'],
}
const rowOf = {}
for (const [row, ids] of Object.entries(ROW)) for (const id of ids) rowOf[id] = row
const got = {}
for (const line of fs.readFileSync(recFile, 'utf8').split('\n').filter(Boolean)) {
  const r = JSON.parse(line)
  if (!r.failures.length) continue
  const text = JSON.stringify(r.failures)
  if (got[r.id] && got[r.id] !== text) throw new Error(`${r.id} differs between runs or engines`)
  got[r.id] = text
}
const out = {}
const missing = []
for (const id of Object.keys(got).sort()) {
  const row = rowOf[id]
  if (!row) { missing.push(id); continue }
  out[id] = { row, why: WHY[row], failures: JSON.parse(got[id]) }
}
for (const id of Object.keys(rowOf)) if (!got[id]) console.log('now passes:', id)
if (missing.length) throw new Error(`unclassified: ${missing.join(' ')}`)
// One case per line block, failures one per line, for readable diffs.
let text = '{\n'
const ids = Object.keys(out)
ids.forEach((id, index) => {
  const k = out[id]
  text += `  ${JSON.stringify(id)}: {\n    "row": ${JSON.stringify(k.row)},\n    "why": ${JSON.stringify(k.why)},\n    "failures": [\n`
  text += k.failures.map(f => `      ${JSON.stringify(f)}`).join(',\n')
  text += `\n    ]\n  }${index + 1 < ids.length ? ',' : ''}\n`
})
text += '}\n'
fs.writeFileSync(outFile, text)
console.log('wrote', ids.length, 'cases')
