import { describe, expect, it } from 'vitest'
import { ATLAS_PALETTE_SIZE, atlasFieldColor } from './atlasPresentation'

describe('atlas field colour', () => {
  it('takes the colour of the field slot, wraps past the palette, and mutes unassigned skills', () => {
    expect(ATLAS_PALETTE_SIZE).toBe(8)
    expect(atlasFieldColor({ id: 'sf_cs', slot: 1 })).toBe('var(--atlas-field-1)')
    expect(atlasFieldColor({ id: 'sf_next', slot: 9 })).toBe('var(--atlas-field-1)')
    expect(atlasFieldColor({ id: null, slot: null })).toBe('var(--app-muted-foreground)')
  })
})
