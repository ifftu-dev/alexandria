// The SCORM 2004 4th Edition test suite's LMS test, as data: its test cases
// (`scorm2004-conformance/adl/cases/*.properties`) read the way the suite's
// own Java reads them, and the evaluation rules its LMS test applies to each
// result. Ported from the suite's `LMSTestCaseGenerator`, `LMSParser`,
// `Command` and `TestCaseEvaluator` (Apache-2.0, see the README beside the
// cases). The browser side, driving the player, is in
// `scorm2004-conformance.spec.ts`.
//
// A case is a list of visits in the order the suite expects the player to
// deliver them. Each visit names the activity (`act` in the lesson's
// address), the calls the lesson makes with the result each must give, the
// user interface questions asked before it starts, and what the tester does
// next (the "current user instructions").

export type AdlMethod =
  | 'Initialize' | 'Terminate' | 'Commit' | 'GetValue' | 'SetValue'
  | 'GetLastError' | 'GetErrorString' | 'GetDiagnostic' | 'compareObjIds'

export interface AdlStep {
  /** The command as written in the case, for messages. */
  source: string
  method: AdlMethod
  /** The call's parameter as the suite builds it: `element!value` for SetValue. */
  param: string
  /** The value the call should return (an evaluation rule may relax it). */
  expected: string
  /** The error code `GetLastError` should report after the call. */
  error: string
}

export interface AdlVisit {
  /** `Act<act>V<visit>`. */
  key: string
  /** The activity: matches `act=` in the lesson's address. */
  act: string
  steps: AdlStep[]
  /** User interface question keys, asked before the lesson starts. */
  questions: string[]
  /** The expected answers, `Y` or `N`, one per question. */
  answers: string[]
  /** What the tester does once the lesson is done (`Continue`, `Exit`, ...). */
  instruction: string
}

export interface AdlCase {
  id: string
  instructions: Record<string, string>
  visits: AdlVisit[]
}

/** A `.properties` file of the suite read as Java's `ResourceBundle` would: `key=value`, `#` comments, `\` continuations. */
export function readBundle(text: string): Map<string, string> {
  const out = new Map<string, string>()
  const lines = text.split(/\r?\n/)
  for (let index = 0; index < lines.length; index++) {
    let line = lines[index]!.replace(/^\s+/, '')
    if (!line || line.startsWith('#') || line.startsWith('!')) continue
    while (line.endsWith('\\') && index + 1 < lines.length) line = line.slice(0, -1) + lines[++index]!.replace(/^\s+/, '')
    const at = line.search(/[=:]/)
    if (at < 0) continue
    out.set(line.slice(0, at).trim(), line.slice(at + 1))
  }
  return out
}

/** `\uXXXX` escapes decoded, every other backslash kept (the generator's `doUnicode`). */
function unicode(value: string): string {
  return value.replace(/\\u([0-9a-fA-F]{4})/g, (_, hex: string) => String.fromCharCode(parseInt(hex, 16)))
}

/** The learner the player reports; the suite expects the one the tester registered. */
export interface AdlLearner { id: string, name: string }

/** A test case file read as the suite's `LMSTestCaseGenerator` reads it. */
export function readCase(id: string, text: string, commands: Map<string, string>, learner: AdlLearner): AdlCase {
  const instructions: Record<string, string> = {}
  const visits: AdlVisit[] = []
  let current: AdlVisit | null = null
  const lines = text.split(/\r?\n/)
  for (let index = 0; index < lines.length; index++) {
    let line = lines[index]!
    if (!line) continue
    // A trailing backslash joins the next line as it is.
    while (line.endsWith('\\') && index + 1 < lines.length) line = line.slice(0, -1) + lines[++index]!
    if (line.includes('####')) continue
    const at = line.indexOf('=')
    if (at < 0) throw new Error(`${id}: no "=" on line ${index + 1}`)
    const key = line.slice(0, at)
    const value = line.slice(at + 1)
    if (['title', 'UserInterAct', 'start', 'purpose', 'RTDInit', 'seqInfo', 'TT', 'end'].includes(key)) {
      instructions[key] = value
      continue
    }
    if (!key.includes('Act')) continue
    const visitKey = key.slice(0, key.indexOf('.'))
    if (!current || current.key !== visitKey) {
      current = { key: visitKey, act: visitKey.slice(3, visitKey.indexOf('V')), steps: [], questions: [], answers: [], instruction: '' }
      visits.push(current)
    }
    if (key.includes('UIQ')) current.questions = value ? value.split('~') : []
    else if (key.includes('UIA')) current.answers = value ? value.split('~') : []
    else if (key.includes('CUI')) current.instruction = value
    else if (key.includes('commands')) {
      const step = decodeCommand(unicode(value), commands)
      if (!step) continue
      // `getLearnerInfoForResultValue`: a read of the learner's name or id
      // expects the learner the tester registered with the LMS.
      const element = value.split('->')[1] ?? ''
      if (step.method === 'GetValue' && (element.includes('LN') || element.includes('LI'))) {
        step.expected = element.includes('LN') ? learner.name : learner.id
      }
      current.steps.push(step)
    }
  }
  return { id, instructions, visits }
}

/** `LMSMessages.Commands.getString`: the bundle's value, or the key itself when it has none. */
function word(commands: Map<string, string>, key: string, argument?: string): string {
  const value = commands.has(key) ? commands.get(key)! : key
  return argument === undefined ? value : value.replace('%s', argument)
}

/** Java's `String.split`, which drops trailing empty strings. */
function split(value: string, separator: string): string[] {
  const parts = value.split(separator)
  while (parts.length > 1 && parts[parts.length - 1] === '') parts.pop()
  return parts
}

/** `setFinalSplitArray`: the last segment of a SetValue's element, split at its first `!`. */
function finalSplit(segment: string): [string, string] {
  if (segment.indexOf('!') === segment.length - 1) return [segment.slice(0, -1), '']
  const at = segment.indexOf('!')
  return at < 0 ? [segment, ''] : [segment.slice(0, at), segment.slice(at + 1)]
}

const METHODS: Record<string, AdlMethod> = {
  I: 'Initialize', T: 'Terminate', C: 'Commit', GET: 'GetValue', SET: 'SetValue',
  GES: 'GetErrorString', GLE: 'GetLastError', GeDi: 'GetDiagnostic', COI: 'compareObjIds',
}

/** One command, `METHOD->arg->expected->error`, decoded as `LMSParser` does. */
export function decodeCommand(source: string, commands: Map<string, string>): AdlStep | null {
  const parts = source.split('->')
  const code = parts[0]!
  const method = METHODS[code]
  // `parseCommands` passes over a command it does not know (DMI has one
  // with no method).
  if (!method) return null
  const R = (key: string, argument?: string) => word(commands, key, argument)
  const step: AdlStep = { source, method, param: '', expected: '', error: '' }

  if (code === 'GET' || code === 'I' || code === 'C') {
    const key = parts[1] ?? ''
    if (key.includes('~')) {
      const segments = split(key, '~')
      if (segments[1] === 'INTR' && key.includes('!')) {
        // An interaction element with its type after `!`: the type only
        // tells the evaluator how to compare.
        for (let index = 0; index < segments.length; index++) {
          if (segments[index]!.includes('!')) segments[index] = segments[index]!.split('!')[0]!
        }
      }
      if (segments.length === 6 && key.includes('a~n~r')) {
        step.param = [R(segments[0]!), R(segments[1]!), R(segments[2]!), R(segments[3]!), R(segments[4]!, segments[5]!)].join('.')
      } else {
        step.param = segments.map(segment => R(segment)).join('.')
      }
    } else {
      step.param = key
    }
    for (let index = 2; index < parts.length - 1; index++) {
      const token = parts[index]!
      step.expected = R(token) === '' && token !== '' ? token : R(token)
    }
    step.error = parts.length > 2 ? parts[parts.length - 1]! : ''
    return step
  }
  if (code === 'SET') {
    const key = parts[1] ?? ''
    const segments = split(key, '~')
    let element = R(segments[0]!)
    if (segments.length === 2) {
      const [last, value] = finalSplit(segments[1]!)
      element += `.${R(last)}!${R(value)}`
    } else if (segments.length === 3) {
      const [last, value] = finalSplit(segments[2]!)
      element += `.${R(segments[1]!)}.${R(last)}!${R(value)}`
    } else if (segments.length === 4) {
      const [last, value] = finalSplit(segments[3]!)
      element += `.${R(segments[1]!)}.${segments[2]}.${R(last)}!${R(value)}`
    } else if (segments.length === 5) {
      if (key.includes('a~n~r')) {
        const [name, request] = segments[2]!.split('!') as [string, string]
        element += `.${R(segments[1]!)}.${R(name)}!${R(request, segments[3]!)}${R(segments[4]!)}`
      } else {
        const [name, value] = segments[4]!.split('!') as [string, string]
        element += `.${R(segments[1]!)}.${R(segments[2]!)}.${R(segments[3]!)}.${R(name)}!${R(value)}`
      }
    } else if (segments.length === 6) {
      const [last, value] = finalSplit(segments[5]!)
      element += `.${[1, 2, 3, 4].map(index => R(segments[index]!)).join('.')}.${R(last)}!${R(value)}`
    }
    step.param = element
    for (let index = 2; index < parts.length - 1; index++) step.expected = R(parts[index]!)
    step.error = parts[parts.length - 1]!
    return step
  }
  if (code === 'GES' || code === 'GeDi') {
    step.param = parts[1] ?? ''
    step.expected = parts[2] ?? ''
    step.error = parts[3] ?? ''
    return step
  }
  if (code === 'GLE') {
    step.expected = parts[2] ?? ''
    step.error = parts[3] ?? ''
    return step
  }
  if (code === 'COI') {
    step.param = parts[1] ?? ''
    return step
  }
  // Terminate: the suite expects nothing of it.
  return step
}

/** What a call returned in the lesson, or why it could not be made. */
export interface AdlResult { value: string, error: string, index: number, failed?: string, ids?: string[] }

/** Seconds in an ISO 8601 duration, or null. */
function durationSeconds(text: string): number | null {
  const match = /^P(?:(\d+)Y)?(?:(\d+)M)?(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?)?$/.exec(text)
  if (!match || text === 'P' || text.endsWith('T')) return null
  const [years, months, days, hours, minutes, seconds] = match.slice(1).map(part => Number(part ?? 0)) as number[]
  return years! * 31536000 + months! * 2592000 + days! * 86400 + hours! * 3600 + minutes! * 60 + seconds!
}

/**
 * Two values of one data model element, compared the way the suite's data
 * model compares them (`DMElement.equals`): exactly, as numbers, as
 * durations or as instants.
 */
export function sameValue(expected: string, returned: string): boolean {
  if (expected === returned) return true
  const number = /^-?\d+(\.\d+)?$/
  if (number.test(expected) && number.test(returned)) return Number(expected) === Number(returned)
  const a = durationSeconds(expected)
  const b = durationSeconds(returned)
  if (a !== null && b !== null) return Math.abs(a - b) < 0.005
  const instant = /^\d{4}(-\d{2}(-\d{2}(T\d{2}(:\d{2}(:\d{2}(\.\d+)?)?)?)?)?)?(Z|[+-]\d{2}(:\d{2})?)?$/
  if (instant.test(expected) && instant.test(returned)) return instantMs(expected) === instantMs(returned)
  return false
}

/**
 * An instant in milliseconds, a time with no zone read as UTC so the result
 * is the same on every machine (Date.parse would read it as local time).
 */
function instantMs(text: string): number {
  const zoneless = text.includes('T') && !/(Z|[+-]\d{2}(:\d{2})?)$/.test(text)
  return Date.parse(zoneless ? `${text}Z` : text)
}

/**
 * Why one result fails the suite's evaluation (`evaluateCurrentResult`), or
 * null when it passes.
 */
export function evaluate(step: AdlStep, result: AdlResult): string | null {
  // Long values (the suite tests limits of thousands of characters) are
  // shortened in messages.
  const show = (value: string) => value.length > 80 ? `${JSON.stringify(value.slice(0, 60))}... (${value.length} characters)` : JSON.stringify(value)
  const call = `${step.method}(${show(step.param)})`
  const errorCheck = () => result.error === step.error ? null : `${call}: error ${result.error}, expected ${step.error}`
  const valueCheck = (ok: boolean) => ok ? errorCheck() : `${call}: returned ${show(result.value)}, expected ${show(step.expected)}`
  if (result.failed) return `${call}: ${result.failed}`
  if (step.method === 'compareObjIds') {
    const expected = step.param.split(',')
    const ids = result.ids ?? []
    return ids.length === expected.length && ids.every(id => expected.includes(id))
      ? null
      : `${call}: objectives ${JSON.stringify(ids)}, expected ${JSON.stringify(expected)}`
  }
  const get = step.method === 'GetValue'
  const requestValid = (value: string) => ['true', 'false', 'unknown'].includes(value)
  if (get && step.param.includes('learner_name')) return errorCheck()
  if (get && (step.param.includes('request_valid.choice') || step.param.includes('request_valid.jump'))) {
    return requestValid(result.value) ? null : `${call}: returned ${JSON.stringify(result.value)}, not true, false or unknown`
  }
  if (get && !step.param.includes('_children') && step.param.includes('comments_from_lms')) {
    const ok = ['0', '301', '403'].includes(result.error) || (step.param === 'cmi.comments_from_lms' && result.error === '401')
    return ok ? null : `${call}: error ${result.error}`
  }
  // A list of children is the same list in any order.
  const sameSet = (a: string, b: string) => a.split(',').sort().join(',') === b.split(',').sort().join(',')
  const regular = () => step.param.includes('request_valid') ? requestValid(result.value)
    : step.param.endsWith('._children') ? sameSet(step.expected, result.value)
      : sameValue(step.expected, result.value)
  if (get && step.expected !== '' && !step.param.includes('_count')) return valueCheck(regular())
  if (get && step.param.includes('&') && step.param.includes('.id')) {
    const id = step.param.slice(step.param.indexOf('&') + 1, step.param.lastIndexOf('&'))
    return valueCheck(result.value === id)
  }
  if (get && result.value.startsWith('{') && !step.param.includes('_count')) return valueCheck(regular())
  if (step.method === 'GetErrorString') {
    const ok = step.expected === 'less255' ? result.value.length < 255
      : step.expected === 'emptyCS' ? result.value === ''
        : result.value === step.expected
    return valueCheck(ok)
  }
  if (step.method === 'GetDiagnostic') return valueCheck(result.value.length < 255)
  return valueCheck(result.value === step.expected)
}
