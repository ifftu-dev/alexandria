import type { Chapter, Course, Element, Package, PackageDescription } from '@/types'
import { packageBackend, packageDescriptionOf, type PackageFixtureName } from './packageBackend'

const HASH = 'ab'.repeat(32)

/** File picker paths the fixture knows: one good package, one per refusal code. */
export const GOOD_PACKAGE = '/packages/workshop-safety.zip'
export const REFUSAL_CODES = [
  'not_an_archive', 'no_manifest', 'manifest_unparseable', 'unsupported_standard',
  'launch_resource_missing', 'launch_resource_remote', 'unsafe_entry_path', 'limit_exceeded',
] as const

/**
 * A fake host for the course editor: an instructor's draft course with a text
 * lesson and a package element over a player fixture package (the SCORM 1.2
 * player fixture unless `fixture` names another). Importing answers from a
 * table of file paths; a preview is served by the player fixture's backend,
 * so the preview really launches the package. Every command is recorded.
 */
export function courseEditorBackend(options: { fixture?: PackageFixtureName } = {}) {
  const fixture = options.fixture ?? 'package-player'
  const player = packageBackend({ fixture })
  const packageDescription: PackageDescription = packageDescriptionOf(fixture)
  const stored: Package = { hash: HASH, standard: packageDescription.standard, title: packageDescription.title, description: packageDescription,
    archive_bytes: 4096, location: 'draft', imported_at: '2026-11-01T00:00:00Z', last_opened_at: null }
  const course: Course = {
    id: 'course-induction', title: 'Workshop induction', kind: 'course', description: null, author_address: 'stake-instructor',
    author_name: 'Fixture instructor', content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: [], skill_ids: [],
    version: 1, status: 'draft', published_at: null, on_chain_tx: null, created_at: '2026-11-01', updated_at: '2026-11-01',
  }
  const chapter: Chapter = { id: 'chapter-1', course_id: course.id, title: 'Before the floor', description: null, position: 0 }
  const elements: Element[] = [
    { id: 'element-text', chapter_id: chapter.id, title: 'House rules', element_type: 'text', content_cid: null,
      content_inline: 'Read this first.', position: 0, duration_seconds: null },
    { id: 'element-package', chapter_id: chapter.id, title: 'Workshop safety', element_type: 'package', content_cid: HASH,
      content_inline: null, position: 1, duration_seconds: null },
  ]
  const imported = new Set<string>([HASH])
  const calls: { command: string, args: unknown }[] = []
  const control = { nextPath: GOOD_PACKAGE as string | null, refuseUpdate: false }

  function importPackage(path: unknown): Package {
    if (path === GOOD_PACKAGE) {
      imported.add(HASH)
      return structuredClone(stored)
    }
    const code = REFUSAL_CODES.find(item => path === `/packages/${item}.zip`)
    if (code) throw new Error(`${code}: what the importer found in ${String(path)}`)
    throw new Error(`package files: cannot read ${String(path)}`)
  }

  async function invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
    calls.push({ command, args: command === 'package_launch_open' ? { hash: args.hash } : structuredClone(args) })
    switch (command) {
      case 'plugin:dialog|open': return control.nextPath
      case 'get_course': return args.courseId === course.id ? course : null
      case 'studio_get_course':
        return { id: `studio-${course.id}`, revision: 1, value: { audience: '', prerequisites: '', outcome: '', initial_prompt: '',
          sources: [], tutor: { enabled: false, guidance: 'socratic', initial_prompt: '' } } }
      case 'studio_list_lesson_feedback': return []
      case 'get_course_completion_policy': return null
      case 'list_chapters': return args.courseId === course.id ? [chapter] : []
      case 'list_elements': return elements.filter(item => item.chapter_id === args.chapterId).map(item => ({ ...item }))
      case 'list_element_skill_tags':
      case 'list_skills': return []
      case 'package_import': return importPackage(args.path)
      case 'package_get': return imported.has(String(args.hash)) ? structuredClone(stored) : null
      case 'create_element': {
        const req = args.req as { title: string, element_type: string, content_hash?: string | null }
        if (req.element_type === 'package' && req.content_hash && !imported.has(req.content_hash)) {
          throw new Error(`not_a_package: no imported package has the hash ${req.content_hash}`)
        }
        const element: Element = { id: `element-${elements.length + 1}`, chapter_id: String(args.chapterId), title: req.title,
          element_type: req.element_type, content_cid: req.content_hash ?? null, content_inline: null, position: elements.length,
          duration_seconds: null }
        elements.push(element)
        return { ...element }
      }
      case 'update_element': {
        if (control.refuseUpdate) {
          throw new Error('package_result_self_reported: a package element carries no grading configuration')
        }
        const element = elements.find(item => item.id === args.elementId)
        if (!element) throw new Error('element not found')
        const req = args.req as { title?: string, content_hash?: string }
        if (req.title) element.title = req.title
        if (req.content_hash) element.content_cid = req.content_hash
        return { ...element }
      }
      // A preview runs on the player fixture's attempt rules, through the
      // preview commands only.
      case 'package_preview_open': return player.invoke('package_attempt_open', args)
      case 'package_preview_fresh': return player.invoke('package_attempt_fresh', args)
      case 'package_preview_initialise': return player.invoke('package_attempt_initialise', args)
      case 'package_preview_commit': return player.invoke('package_attempt_commit', args)
      case 'package_preview_activities': return player.invoke('package_attempt_activities', args)
      case 'package_preview_terminate': return player.invoke('package_attempt_terminate', args)
      case 'package_preview_sequencing': return player.invoke('package_attempt_sequencing', args)
      case 'package_preview_sequencing_save': return player.invoke('package_attempt_sequencing_save', args)
      default: return player.invoke(command, args)
    }
  }

  return {
    invoke,
    control,
    called: (command: string) => calls.filter(call => call.command === command),
  }
}
