import { computed, onUnmounted, ref } from 'vue'

interface SpeechRecognitionAlternativeLike {
  transcript: string
  confidence: number
}

interface SpeechRecognitionResultLike {
  isFinal: boolean
  item(index: number): SpeechRecognitionAlternativeLike
}

interface SpeechRecognitionResultListLike {
  length: number
  item(index: number): SpeechRecognitionResultLike
}

interface SpeechRecognitionEventLike extends Event {
  resultIndex: number
  results: SpeechRecognitionResultListLike
}

interface SpeechRecognitionErrorEventLike extends Event {
  error: string
  message: string
}

interface SpeechRecognitionLike extends EventTarget {
  continuous: boolean
  interimResults: boolean
  lang: string
  processLocally?: boolean
  onresult: ((event: SpeechRecognitionEventLike) => void) | null
  onerror: ((event: SpeechRecognitionErrorEventLike) => void) | null
  onend: (() => void) | null
  start(): void
  stop(): void
  abort(): void
}

type SpeechRecognitionConstructor = new () => SpeechRecognitionLike

interface SpeechRecognitionWindow {
  SpeechRecognition?: SpeechRecognitionConstructor
  webkitSpeechRecognition?: SpeechRecognitionConstructor
}

export interface LocalSpeechSegment {
  text: string
  confidence: number
  isFinal: boolean
}

/**
 * Privacy-safe wrapper around the browser speech API.
 *
 * Recognition is exposed only when the WebView supports the explicit
 * `processLocally` control. Engines that may upload audio are intentionally
 * treated as unavailable; callers can keep the manual transcript fallback.
 */
export function useLocalSpeechRecognition() {
  const recognition = ref<SpeechRecognitionLike | null>(null)
  const listening = ref(false)
  const interimText = ref('')
  const error = ref<string | null>(null)
  const onSegment = ref<((segment: LocalSpeechSegment) => void) | null>(null)

  const constructor = computed<SpeechRecognitionConstructor | null>(() => {
    if (typeof window === 'undefined') return null
    const speechWindow = window as unknown as SpeechRecognitionWindow
    return speechWindow.SpeechRecognition ?? speechWindow.webkitSpeechRecognition ?? null
  })

  const locallyAvailable = computed(() => {
    const Recognition = constructor.value
    if (!Recognition) return false
    try {
      const probe = new Recognition()
      const supported = 'processLocally' in probe
      probe.abort()
      return supported
    } catch {
      return false
    }
  })

  function stop() {
    const active = recognition.value
    recognition.value = null
    listening.value = false
    interimText.value = ''
    if (active) active.stop()
  }

  function start(
    callback: (segment: LocalSpeechSegment) => void,
    language = 'en-US',
  ): boolean {
    if (listening.value) return true
    const Recognition = constructor.value
    if (!Recognition) {
      error.value = 'Speech recognition is not available in this WebView.'
      return false
    }
    const instance = new Recognition()
    if (!('processLocally' in instance)) {
      instance.abort()
      error.value = 'This WebView cannot guarantee on-device transcription.'
      return false
    }

    instance.processLocally = true
    instance.continuous = true
    instance.interimResults = true
    instance.lang = language
    onSegment.value = callback
    error.value = null

    instance.onresult = (event) => {
      let interim = ''
      for (let index = event.resultIndex; index < event.results.length; index += 1) {
        const result = event.results.item(index)
        const alternative = result.item(0)
        const text = alternative.transcript.trim()
        if (!text) continue
        if (result.isFinal) {
          onSegment.value?.({ text, confidence: alternative.confidence, isFinal: true })
        } else {
          interim += `${text} `
        }
      }
      interimText.value = interim.trim()
    }
    instance.onerror = (event) => {
      error.value = event.message || event.error
    }
    instance.onend = () => {
      recognition.value = null
      listening.value = false
      interimText.value = ''
    }

    recognition.value = instance
    try {
      instance.start()
      listening.value = true
      return true
    } catch (cause) {
      recognition.value = null
      error.value = cause instanceof Error ? cause.message : String(cause)
      return false
    }
  }

  onUnmounted(() => {
    const active = recognition.value
    recognition.value = null
    if (active) active.abort()
  })

  return {
    locallyAvailable,
    listening,
    interimText,
    error,
    start,
    stop,
  }
}
