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

/** The version-3 statement's line, for surfaces that know the rung but hold
 *  no statement (a hosted attempt's result, a request's policy, before any
 *  credential exists). The statement's own text, verbatim. */
export const SECOND_DEVICE_NOT_RULED_OUT
  = 'Whether a second device or person helped: not ruled out. Only a live segment, where a person asks unscripted questions, rules it out.'

/** The second-device line for a rung: the statement's text below `live`
 *  (a missing rung included), `null` at `live`. */
export function secondDeviceNote(rung: string | null | undefined): string | null {
  return rung === 'live' ? null : SECOND_DEVICE_NOT_RULED_OUT
}
