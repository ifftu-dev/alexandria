import { describe, expect, it } from 'vitest'
import { keyboardViewport } from './keyboardViewport'

describe('keyboardViewport', () => {
  it('is closed when the visual viewport fills the layout viewport', () => {
    expect(keyboardViewport(874, 874)).toEqual({ open: false, height: 874 })
  })

  it('ignores small changes such as a collapsing browser bar', () => {
    expect(keyboardViewport(874, 800).open).toBe(false)
  })

  it('is open when the keyboard takes the lower part of the screen', () => {
    expect(keyboardViewport(874, 487.6)).toEqual({ open: true, height: 488 })
  })
})
