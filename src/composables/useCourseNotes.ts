// Course notes — the learner's notes on a course's lessons.
//
// Notes arrive from chat: a saved chat note that cites a lesson can be copied
// into that course. `revision` moves whenever a note is added or deleted
// anywhere in the app, so an open course player refreshes its list.

import { ref } from 'vue'

import { useLocalApi } from './useLocalApi'
import type { CourseNote } from '@/types'

const revision = ref(0)

export function useCourseNotes() {
  const { invoke } = useLocalApi()

  function list(courseId: string): Promise<CourseNote[]> {
    return invoke<CourseNote[]>('course_notes_list', { courseId })
  }

  async function remove(noteId: string): Promise<void> {
    await invoke('course_note_delete', { noteId })
    revision.value++
  }

  /** Copies a saved chat note into the course of the lesson it cites. */
  async function saveFromChat(artifactId: string): Promise<CourseNote> {
    const note = await invoke<CourseNote>('chat_save_course_note', { artifactId })
    revision.value++
    return note
  }

  return { revision, list, remove, saveFromChat }
}
