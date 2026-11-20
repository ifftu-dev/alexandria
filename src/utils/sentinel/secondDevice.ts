// The line a coverage statement below the `live` rung carries (version 3 on)
// saying a second device or person was not ruled out. It is the statement's
// own fixed text, quoted rather than translated, so surfaces can show it
// beside the rung without opening the whole statement.
import type { CoverageStatement } from '@/types'

const SECOND_DEVICE_LINE = 'Whether a second device or person helped:'

/** The statement's second-device line, or `null` when it has none (`live`,
 *  or a version from before the line existed). */
export function secondDeviceLine(statement: CoverageStatement | null | undefined): string | null {
  return statement?.not_checked.find(line => line.startsWith(SECOND_DEVICE_LINE)) ?? null
}
