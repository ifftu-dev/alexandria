// Below `live`, every surface says a second device or person was not ruled
// out, in the coverage statement's own words.
import { describe, expect, it } from 'vitest'
import { secondDeviceNote } from '@/utils/sentinel/secondDevice'

const LINE = 'Whether a second device or person helped: not ruled out. Only a live segment, where a person asks unscripted questions, rules it out.'

describe('secondDeviceNote', () => {
  it('quotes the statement line below the live rung, and says nothing at live', () => {
    for (const rung of ['none', 'process', 'probed', 'consistent', null, undefined]) {
      expect(secondDeviceNote(rung)).toBe(LINE)
    }
    expect(secondDeviceNote('live')).toBeNull()
  })
})
