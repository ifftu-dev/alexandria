import { ref, readonly } from 'vue'
import type {
  Classroom,
  ClassroomChannel,
  ClassroomMember,
  ClassroomMessage,
  JoinRequest,
  ClassroomCall,
  ClassroomMessageEvent,
  ClassroomMetaEvent,
} from '@/types'
import { useLocalApi } from './useLocalApi'
import { onProfileLocked } from './useProfiles'

const { invoke } = useLocalApi()

// ── Module-level singleton state ───────────────────────────────────

const classrooms = ref<Classroom[]>([])
const currentClassroom = ref<Classroom | null>(null)
const channels = ref<ClassroomChannel[]>([])
const members = ref<ClassroomMember[]>([])

/** Map of channelId → message array (newest last). */
const messages = ref<Record<string, ClassroomMessage[]>>({})

const joinRequests = ref<JoinRequest[]>([])
const activeCall = ref<ClassroomCall | null>(null)
const loading = ref(false)
const lastError = ref<string | null>(null)

let messageUnlisten: (() => void) | null = null
let metaUnlisten: (() => void) | null = null
let listenerSetup: Promise<void> | null = null
let profileGeneration = 0

function isCurrentProfile(generation: number): boolean {
  return generation === profileGeneration
}

// ── Tauri event setup ──────────────────────────────────────────────

async function setupEventListeners() {
  if (messageUnlisten && metaUnlisten) return
  if (listenerSetup) return listenerSetup

  const generation = profileGeneration
  const setup = (async () => {
    const { listen } = await import('@tauri-apps/api/event')
    if (!isCurrentProfile(generation)) return

    let nextMessageUnlisten: (() => void) | null = null
    let nextMetaUnlisten: (() => void) | null = null
    try {
      nextMessageUnlisten = await listen<ClassroomMessageEvent>('classroom:message', (event) => {
        if (!isCurrentProfile(generation)) return
        const { channel_id, message } = event.payload
        const current = messages.value[channel_id] ?? []
        // Deduplicate by id
        if (!current.find((m) => m.id === message.id)) {
          messages.value = {
            ...messages.value,
            [channel_id]: [
              ...current,
              {
                id: message.id,
                channel_id: message.channel_id,
                classroom_id: message.classroom_id,
                sender_address: message.sender_address,
                sender_name: message.sender_name,
                content: message.content,
                deleted: false,
                edited_at: null,
                sent_at: message.sent_at,
                received_at: new Date().toISOString(),
              },
            ],
          }
        }
      })
      if (!isCurrentProfile(generation)) return

      nextMetaUnlisten = await listen<ClassroomMetaEvent>('classroom:meta', (event) => {
        if (!isCurrentProfile(generation)) return
        const { event_type, data } = event.payload

        switch (event_type) {
          case 'MemberApproved': {
            const d = data as { stake_address: string; display_name?: string | null }
            if (!members.value.find((m) => m.stake_address === d.stake_address)) {
              members.value = [
                ...members.value,
                {
                  classroom_id: event.payload.classroom_id,
                  stake_address: d.stake_address,
                  role: 'member',
                  display_name: d.display_name ?? null,
                  joined_at: new Date().toISOString(),
                },
              ]
            }
            joinRequests.value = joinRequests.value.filter(
              (r) => r.stake_address !== d.stake_address,
            )
            break
          }
          case 'MemberLeft':
          case 'MemberKicked': {
            const d = data as { stake_address: string }
            members.value = members.value.filter((m) => m.stake_address !== d.stake_address)
            break
          }
          case 'RoleChanged': {
            const d = data as { stake_address: string; new_role: string }
            members.value = members.value.map((m) =>
              m.stake_address === d.stake_address
                ? { ...m, role: d.new_role as 'owner' | 'moderator' | 'member' }
                : m,
            )
            break
          }
          case 'JoinRequest': {
            const d = data as {
              request_id: string
              display_name?: string | null
              message?: string | null
            }
            if (!joinRequests.value.find((r) => r.id === d.request_id)) {
              joinRequests.value = [
                ...joinRequests.value,
                {
                  id: d.request_id,
                  classroom_id: event.payload.classroom_id,
                  stake_address: '',
                  display_name: d.display_name ?? null,
                  message: d.message ?? null,
                  status: 'pending',
                  reviewed_by: null,
                  requested_at: new Date().toISOString(),
                  reviewed_at: null,
                },
              ]
            }
            break
          }
          case 'CallStarted': {
            const d = data as { call_id: string; ticket: string; started_by: string }
            activeCall.value = {
              id: d.call_id,
              classroom_id: event.payload.classroom_id,
              channel_id: null,
              title: 'Voice Call',
              ticket: d.ticket,
              started_by: d.started_by,
              status: 'active',
              started_at: new Date().toISOString(),
              ended_at: null,
            }
            break
          }
          case 'CallEnded': {
            activeCall.value = null
            break
          }
        }
      })
      if (!isCurrentProfile(generation)) return

      messageUnlisten = nextMessageUnlisten
      metaUnlisten = nextMetaUnlisten
      nextMessageUnlisten = null
      nextMetaUnlisten = null
    } finally {
      nextMessageUnlisten?.()
      nextMetaUnlisten?.()
    }
  })()
  listenerSetup = setup
  try {
    await setup
  } finally {
    if (listenerSetup === setup) listenerSetup = null
  }
}

function teardownEventListeners() {
  if (messageUnlisten) {
    messageUnlisten()
    messageUnlisten = null
  }
  if (metaUnlisten) {
    metaUnlisten()
    metaUnlisten = null
  }
}

function resetForProfileLock(): void {
  profileGeneration++
  listenerSetup = null
  classrooms.value = []
  currentClassroom.value = null
  channels.value = []
  members.value = []
  messages.value = {}
  joinRequests.value = []
  activeCall.value = null
  loading.value = false
  lastError.value = null
  teardownEventListeners()
}

onProfileLocked(resetForProfileLock)

// ── Public API ─────────────────────────────────────────────────────

async function loadClassrooms() {
  const generation = profileGeneration
  loading.value = true
  lastError.value = null
  try {
    const loaded = await invoke<Classroom[]>('classroom_list')
    if (isCurrentProfile(generation)) classrooms.value = loaded
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
  } finally {
    if (isCurrentProfile(generation)) loading.value = false
  }
}

async function createClassroom(
  name: string,
  description?: string,
  iconEmoji?: string,
): Promise<Classroom | null> {
  const generation = profileGeneration
  try {
    const classroom = await invoke<Classroom>('classroom_create', {
      name,
      description: description ?? null,
      iconEmoji: iconEmoji ?? null,
    })
    if (!isCurrentProfile(generation)) return null
    classrooms.value = [...classrooms.value, classroom]
    return classroom
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    return null
  }
}

async function enterClassroom(id: string) {
  const generation = profileGeneration
  loading.value = true
  lastError.value = null
  try {
    await setupEventListeners()
    if (!isCurrentProfile(generation)) return

    const [classroom, chans, mems] = await Promise.all([
      invoke<Classroom>('classroom_get', { classroomId: id }),
      invoke<ClassroomChannel[]>('classroom_list_channels', { classroomId: id }),
      invoke<ClassroomMember[]>('classroom_list_members', { classroomId: id }),
    ])

    if (!isCurrentProfile(generation)) return
    currentClassroom.value = classroom
    channels.value = chans
    members.value = mems

    // Subscribe to P2P topics
    await invoke('classroom_subscribe', { classroomId: id }).catch(() => {})
    if (!isCurrentProfile(generation)) return

    // Load active call if any
    const call = await invoke<ClassroomCall | null>('classroom_get_active_call', {
      classroomId: id,
    })
    if (!isCurrentProfile(generation)) return
    activeCall.value = call

    // Load join requests if moderator/owner
    if (classroom.my_role === 'owner' || classroom.my_role === 'moderator') {
      const requests = await invoke<JoinRequest[]>('classroom_list_join_requests', {
        classroomId: id,
      })
      if (isCurrentProfile(generation)) joinRequests.value = requests
    }
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
  } finally {
    if (isCurrentProfile(generation)) loading.value = false
  }
}

async function exitClassroom() {
  const generation = profileGeneration
  if (currentClassroom.value) {
    await invoke('classroom_unsubscribe', { classroomId: currentClassroom.value.id }).catch(
      () => {},
    )
  }
  if (!isCurrentProfile(generation)) return
  currentClassroom.value = null
  channels.value = []
  members.value = []
  messages.value = {}
  joinRequests.value = []
  activeCall.value = null
  teardownEventListeners()
}

async function loadMessages(channelId: string, beforeId?: string) {
  const generation = profileGeneration
  try {
    const msgs = await invoke<ClassroomMessage[]>('classroom_get_messages', {
      channelId,
      beforeId: beforeId ?? null,
      limit: 50,
    })
    if (!isCurrentProfile(generation)) return
    // Merge with existing (avoid duplicates)
    const existing = messages.value[channelId] ?? []
    const existingIds = new Set(existing.map((m) => m.id))
    const newMsgs = msgs.filter((m) => !existingIds.has(m.id))
    messages.value = {
      ...messages.value,
      [channelId]: beforeId
        ? [...newMsgs, ...existing] // prepend older messages
        : [...existing, ...newMsgs],
    }
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
  }
}

async function sendMessage(
  channelId: string,
  content: string,
): Promise<ClassroomMessage | null> {
  const generation = profileGeneration
  try {
    const msg = await invoke<ClassroomMessage>('classroom_send_message', {
      channelId,
      content,
    })
    if (!isCurrentProfile(generation)) return null
    const current = messages.value[channelId] ?? []
    if (!current.find((m) => m.id === msg.id)) {
      messages.value = {
        ...messages.value,
        [channelId]: [...current, msg],
      }
    }
    return msg
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    return null
  }
}

async function deleteMessage(messageId: string, channelId: string) {
  const generation = profileGeneration
  try {
    await invoke('classroom_delete_message', { messageId })
    if (!isCurrentProfile(generation)) return
    messages.value = {
      ...messages.value,
      [channelId]: (messages.value[channelId] ?? []).map((m) =>
        m.id === messageId ? { ...m, deleted: true, content: '[deleted]' } : m,
      ),
    }
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
  }
}

async function requestJoin(classroomId: string, message?: string) {
  const generation = profileGeneration
  try {
    await invoke('classroom_request_join', {
      classroomId,
      message: message ?? null,
    })
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    throw e
  }
}

async function approveRequest(classroomId: string, stakeAddress: string) {
  const generation = profileGeneration
  try {
    await invoke('classroom_approve_member', { classroomId, stakeAddress })
    if (!isCurrentProfile(generation)) return
    joinRequests.value = joinRequests.value.filter(
      (r) => !(r.classroom_id === classroomId && r.stake_address === stakeAddress),
    )
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    throw e
  }
}

async function denyRequest(classroomId: string, stakeAddress: string) {
  const generation = profileGeneration
  try {
    await invoke('classroom_deny_member', { classroomId, stakeAddress })
    if (!isCurrentProfile(generation)) return
    joinRequests.value = joinRequests.value.filter(
      (r) => !(r.classroom_id === classroomId && r.stake_address === stakeAddress),
    )
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    throw e
  }
}

async function leaveClassroom(classroomId: string) {
  const generation = profileGeneration
  try {
    await invoke('classroom_leave', { classroomId })
    if (!isCurrentProfile(generation)) return
    classrooms.value = classrooms.value.filter((c) => c.id !== classroomId)
    if (currentClassroom.value?.id === classroomId) {
      await exitClassroom()
    }
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    throw e
  }
}

async function createChannel(
  classroomId: string,
  name: string,
  description?: string,
  channelType?: string,
): Promise<ClassroomChannel | null> {
  const generation = profileGeneration
  try {
    const channel = await invoke<ClassroomChannel>('classroom_create_channel', {
      classroomId,
      name,
      description: description ?? null,
      channelType: channelType ?? 'text',
    })
    if (!isCurrentProfile(generation)) return null
    channels.value = [...channels.value, channel].sort((a, b) => a.position - b.position)
    return channel
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    return null
  }
}

async function startCall(classroomId: string): Promise<ClassroomCall | null> {
  const generation = profileGeneration
  try {
    const call = await invoke<ClassroomCall>('classroom_start_call', {
      classroomId,
      channelId: null,
      displayName: null,
      cameraId: null,
      micId: null,
      speakerId: null,
    })
    if (!isCurrentProfile(generation)) return null
    activeCall.value = call
    return call
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    return null
  }
}

async function joinCall(callId: string): Promise<void> {
  const generation = profileGeneration
  try {
    await invoke('classroom_join_call', {
      callId,
      displayName: null,
      cameraId: null,
      micId: null,
      speakerId: null,
    })
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    throw e
  }
}

async function endCall(callId: string): Promise<void> {
  const generation = profileGeneration
  try {
    await invoke('classroom_end_call', { callId })
    if (!isCurrentProfile(generation)) return
    activeCall.value = null
  } catch (e) {
    if (isCurrentProfile(generation)) lastError.value = String(e)
    throw e
  }
}

function clearError() {
  lastError.value = null
}

export function useClassroom() {
  return {
    // State (readonly)
    classrooms: readonly(classrooms),
    currentClassroom: readonly(currentClassroom),
    channels: readonly(channels),
    members: readonly(members),
    messages: readonly(messages),
    joinRequests: readonly(joinRequests),
    activeCall: readonly(activeCall),
    loading: readonly(loading),
    lastError: readonly(lastError),

    // Actions
    loadClassrooms,
    createClassroom,
    enterClassroom,
    exitClassroom,
    loadMessages,
    sendMessage,
    deleteMessage,
    requestJoin,
    approveRequest,
    denyRequest,
    leaveClassroom,
    createChannel,
    startCall,
    joinCall,
    endCall,
    clearError,
  }
}
