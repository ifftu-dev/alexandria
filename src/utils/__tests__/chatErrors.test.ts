import { describe, expect, it } from 'vitest'
import en from '@/locales/en'
import { CHAT_ERROR_CODES } from '@/generated/chat-vocab'
import {
  chatErrorCode,
  chatErrorMessageKey,
  parseChatError,
  signInErrorMessageKey,
} from '../chatErrors'

/** Whether a dotted key resolves to a message in the English catalog. */
function translated(key: string): boolean {
  let node: unknown = en
  for (const part of key.split('.')) {
    if (typeof node !== 'object' || node === null || !(part in node)) return false
    node = (node as Record<string, unknown>)[part]
  }
  return typeof node === 'string' && node.length > 0
}

describe('chat error codes', () => {
  it('parses the code before the first colon and keeps the rest as detail', () => {
    expect(parseChatError('invalid_input: write a message first')).toEqual({
      code: 'invalid_input',
      detail: 'write a message first',
    })
    expect(parseChatError(new Error('rate_limited:7'))).toEqual({ code: 'rate_limited', detail: '7' })
    expect(parseChatError('http:503')).toEqual({ code: 'http', detail: '503' })
    expect(parseChatError('guardian_pending')).toEqual({ code: 'guardian_pending', detail: '' })
  })

  it('treats anything the backend does not list as no code', () => {
    expect(parseChatError('Error: something odd').code).toBeNull()
    expect(parseChatError('profile_locked: unlock first').code).toBeNull()
    expect(parseChatError('').code).toBeNull()
    expect(chatErrorCode('weird')).toBe('generic')
  })

  it('round-trips every generated code through the parser', () => {
    for (const code of CHAT_ERROR_CODES) {
      expect(chatErrorCode(`${code}: detail`)).toBe(code)
      expect(chatErrorCode(new Error(code))).toBe(code)
    }
  })

  it('translates every generated code, and the codes the webview adds', () => {
    for (const code of [...CHAT_ERROR_CODES, 'generic', 'memory'] as const) {
      const key = chatErrorMessageKey(code)
      expect(translated(key), `${code} -> ${key}`).toBe(true)
    }
  })

  it('keeps the dedicated messages the panel and the answer showed before', () => {
    expect(chatErrorMessageKey('guardian_pending')).toBe('chat.errors.guardian_pending')
    expect(chatErrorMessageKey('usage_limit')).toBe('chat.errors.usage_limit')
    expect(chatErrorMessageKey('storage_error')).toBe('chat.errors.generic')
    expect(chatErrorMessageKey('no_lesson')).toBe('chat.errors.generic')
  })

  it('translates every sign-in message it knows', () => {
    for (const code of CHAT_ERROR_CODES) {
      const key = signInErrorMessageKey(code)
      if (key !== null) expect(translated(key), `${code} -> ${key}`).toBe(true)
    }
    expect(signInErrorMessageKey('signin_timeout')).toBe('chat.signin.errors.timeout')
    expect(signInErrorMessageKey('model_unavailable')).toBe('chat.signin.errors.noModels')
    expect(signInErrorMessageKey('signin_failed')).toBeNull()
  })
})
