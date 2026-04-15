// Element dispatch registry for the Player.
//
// Phase 0 of the plugin system (see /Users/hack/.claude/plans/prancy-bubbling-grove.md):
// replaces the v-if chain in Player.vue with a typed registry. Phase 1+ will
// register plugin-loaded components through `registerElementBinding` so the
// dispatch path is identical for built-ins and community plugins.

import type { Component } from 'vue'
import type { Element, QuizResult } from '@/types'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import TextContent from '@/components/course/TextContent.vue'
import PdfViewer from '@/components/course/PdfViewer.vue'
import QuizEngine from '@/components/course/QuizEngine.vue'
import McqQuestion from '@/components/course/McqQuestion.vue'
import EssayInput from '@/components/course/EssayInput.vue'
import DownloadableElement from '@/components/course/DownloadableElement.vue'
import InteractivePlaceholder from '@/components/course/InteractivePlaceholder.vue'
import UnknownElement from '@/components/course/UnknownElement.vue'
import PluginHost from '@/components/plugin/PluginHost.vue'

// Context the Player provides to every dispatched element binding. Bindings
// pull whatever they need out of this; the host owns all stateful concerns
// (download lifecycle, completion progress, labels, enrollment) so element
// components stay thin and the same registry shape works for plugins later.
export interface ElementHostContext {
  element: Element
  isCompleted: boolean
  downloading: boolean
  downloadError: string | null
  /** Enrollment the learner is taking this element under. `null` while
   *  browsing without enrolling — graded plugin submissions require an
   *  enrollment id and refuse to grade otherwise. */
  enrollmentId: string | null
  onDownload: () => void
  onComplete: () => void
  onScoredComplete: (score: number) => void
  onQuizComplete: (result: QuizResult) => void
  elementTypeLabel: (type: string) => string
}

export interface ElementBinding {
  component: Component
  props: (ctx: ElementHostContext) => Record<string, unknown>
  events: (ctx: ElementHostContext) => Record<string, (...args: never[]) => void>
}

const builtinRegistry = new Map<string, ElementBinding>([
  ['video', {
    component: VideoPlayer,
    props: ({ element }) => ({
      contentCid: element.content_cid,
      title: element.title,
    }),
    events: ({ onComplete }) => ({ complete: onComplete }),
  }],
  ['text', {
    component: TextContent,
    props: ({ element }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
    }),
    events: ({ onComplete }) => ({ complete: onComplete }),
  }],
  ['pdf', {
    component: PdfViewer,
    props: ({ element }) => ({
      contentCid: element.content_cid,
      pageCount: (element as { page_count?: number }).page_count,
    }),
    events: ({ onComplete }) => ({ complete: onComplete }),
  }],
  ['downloadable', {
    component: DownloadableElement,
    props: ({ element, downloading, downloadError }) => ({
      element,
      downloading,
      error: downloadError,
    }),
    events: ({ onDownload }) => ({ download: onDownload }),
  }],
  ['quiz', {
    component: QuizEngine,
    props: ({ element }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
      elementId: element.id,
    }),
    events: ({ onQuizComplete }) => ({ complete: onQuizComplete as (...args: never[]) => void }),
  }],
  ['assessment', {
    component: QuizEngine,
    props: ({ element }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
      elementId: element.id,
    }),
    events: ({ onQuizComplete }) => ({ complete: onQuizComplete as (...args: never[]) => void }),
  }],
  ['objective_single_mcq', {
    component: McqQuestion,
    props: ({ element, isCompleted }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
      elementId: element.id,
      type: 'objective_single_mcq',
      isCompleted,
    }),
    events: ({ onScoredComplete }) => ({ complete: onScoredComplete as (...args: never[]) => void }),
  }],
  ['objective_multi_mcq', {
    component: McqQuestion,
    props: ({ element, isCompleted }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
      elementId: element.id,
      type: 'objective_multi_mcq',
      isCompleted,
    }),
    events: ({ onScoredComplete }) => ({ complete: onScoredComplete as (...args: never[]) => void }),
  }],
  ['subjective_mcq', {
    component: McqQuestion,
    props: ({ element, isCompleted }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
      elementId: element.id,
      type: 'subjective_mcq',
      isCompleted,
    }),
    events: ({ onScoredComplete }) => ({ complete: onScoredComplete as (...args: never[]) => void }),
  }],
  ['essay', {
    component: EssayInput,
    props: ({ element, isCompleted }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
      elementId: element.id,
      isCompleted,
    }),
    events: ({ onScoredComplete }) => ({ complete: onScoredComplete as (...args: never[]) => void }),
  }],
  ['interactive', {
    component: InteractivePlaceholder,
    props: ({ element }) => ({
      contentCid: element.content_cid,
      contentInline: element.content_inline,
    }),
    events: () => ({}),
  }],
  // Phase 1 of the community plugin system: an element with
  // element_type === 'plugin' and a non-NULL plugin_cid dispatches to a
  // sandboxed iframe via PluginHost. Built-ins remain in this registry
  // unchanged; plugins are an additional dispatch path, not a replacement.
  // Phase 2 wires `enrollmentId` through so graded plugins can persist
  // their submission bundle to element_submissions.
  ['plugin', {
    component: PluginHost,
    props: ({ element, enrollmentId }) => ({
      element,
      mode: 'learn',
      enrollmentId,
    }),
    events: ({ onComplete, onScoredComplete }) => ({
      complete: onComplete,
      'scored-complete': onScoredComplete as (...args: never[]) => void,
    }),
  }],
])

const fallbackBinding: ElementBinding = {
  component: UnknownElement,
  props: ({ element, elementTypeLabel }) => ({
    element,
    label: elementTypeLabel(element.element_type),
  }),
  events: () => ({}),
}

export function resolveElementBinding(elementType: string): ElementBinding {
  return builtinRegistry.get(elementType) ?? fallbackBinding
}

// Future extension point for Phase 1+ plugin registration. Plugins will call
// this with their CID-derived element_type and a binding that mounts a
// PluginIframe. Built-ins can be overridden but doing so should be exceptional.
export function registerElementBinding(elementType: string, binding: ElementBinding): void {
  builtinRegistry.set(elementType, binding)
}
