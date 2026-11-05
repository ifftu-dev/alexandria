import { expect, it } from 'vitest'
import { errorMessage } from '../errorMessage'

it('reads the message of a thrown Error', () => {
  expect(errorMessage(new TypeError('profile is locked'))).toBe('profile is locked')
})

it('shows anything else thrown as text, as a Tauri command rejection arrives', () => {
  expect(errorMessage('not_found: course c-1')).toBe('not_found: course c-1')
  expect(errorMessage(42)).toBe('42')
  expect(errorMessage(null)).toBe('null')
  expect(errorMessage(undefined)).toBe('undefined')
})
