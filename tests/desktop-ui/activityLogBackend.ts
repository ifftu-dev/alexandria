import type { LearningRecordEntry, LearningRecordExport, LearningRecordUsage } from '@/types'

/** The size limit the fixture reports: 16 MiB, as the app's. */
export const FIXTURE_LIMIT_BYTES = 16 * 1024 * 1024

/** The export the fixture hands back, byte for byte. */
export const FIXTURE_EXPORT: LearningRecordExport = {
  statements: '[\n  {\n    "verb": { "id": "http://adlnet.gov/expapi/verbs/completed" }\n  }\n]\n',
  signature: '{\n  "format": "alexandria-record-export/1",\n  "signer": "did:key:z6MkFixture"\n}\n',
}

const COURSE = 'a'.repeat(64)

function entry(index: number, overrides: Partial<LearningRecordEntry> = {}): LearningRecordEntry {
  return {
    id: `record-${index}`,
    kind: 'completion',
    verb: 'launched',
    course_id: COURSE,
    course_title: 'Warehouse safety',
    element_id: null,
    element_title: null,
    activity_id: `did:key:z6MkCreator/courses/${COURSE}`,
    summary: false,
    bytes: 400,
    recorded_at: `2026-11-20T10:${String(59 - (index % 60)).padStart(2, '0')}:00.000Z`,
    percent: null,
    score: null,
    ...overrides,
  }
}

/** Four records, newest first, or `count` launches for the paging case. */
function seed(count: number | null): LearningRecordEntry[] {
  if (count !== null) return Array.from({ length: count }, (_, index) => entry(index))
  return [
    entry(0, { verb: 'passed', element_id: 'quiz', element_title: 'Check', score: 0.8, bytes: 520 }),
    entry(1, { verb: 'responded', kind: 'interaction', element_id: 'quiz', element_title: 'Check', bytes: 610 }),
    entry(2, { verb: 'progressed', percent: 50, summary: true, bytes: 450 }),
    entry(3, { verb: 'registered', course_title: null, bytes: 380 }),
  ]
}

export interface ActivityLogCall {
  command: string
  args?: Record<string, unknown>
}

/** An in-memory activity log behind the four activity log commands. */
export function activityLogBackend(count: number | null) {
  let records = seed(count)
  const calls: ActivityLogCall[] = []
  const usage = (): LearningRecordUsage => ({
    bytes: records.reduce((sum, r) => sum + r.bytes, 0),
    limit_bytes: FIXTURE_LIMIT_BYTES,
    records: records.length,
    interaction_records: records.filter(r => r.kind === 'interaction').length,
  })
  async function invoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
    calls.push({ command, args })
    switch (command) {
      case 'learning_records_usage':
        return usage()
      case 'learning_records_list': {
        const offset = Number(args?.offset ?? 0)
        const limit = Number(args?.limit ?? 0)
        return structuredClone(records.slice(offset, offset + limit))
      }
      case 'learning_records_export':
        return structuredClone(FIXTURE_EXPORT)
      case 'learning_records_delete': {
        const ids = new Set((args?.ids as string[] | undefined) ?? [])
        const before = records.length
        records = records.filter(r => !ids.has(r.id))
        return before - records.length
      }
      case 'learning_records_delete_all': {
        const before = records.length
        records = []
        return before
      }
      default:
        throw new Error(`Unexpected fixture IPC command: ${command}`)
    }
  }
  return { invoke, calls }
}
