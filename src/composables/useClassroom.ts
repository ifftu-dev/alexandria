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

// ── Tauri event setup ──────────────────────────────────────────────

async function setupEventListeners() {
  if (messageUnlisten) return

  const { listen } = await import('@tauri-apps/api/event')

  messageUnlisten = await listen<ClassroomMessageEvent>('classroom:message', (event) => {
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

  metaUnlisten = await listen<ClassroomMetaEvent>('classroom:meta', (event) => {
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
        // Remove from join requests
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
          m.stake_address === d.stake_address ? { ...m, role: d.new_role as 'owner' | 'moderator' | 'member' } : m,
        )
        break
      }
      case 'JoinRequest': {
        const d = data as { request_id: string; display_name?: string | null; message?: string | null }
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

// ── Public API ─────────────────────────────────────────────────────

async function loadClassrooms() {
  loading.value = true
  lastError.value = null
  try {
    classrooms.value = await invoke<Classroom[]>('classroom_list')
  } catch (e) {
    lastError.value = String(e)
  } finally {
    loading.value = false
  }
}

async function createClassroom(
  name: string,
  description?: string,
  iconEmoji?: string,
): Promise<Classroom | null> {
  try {
    const classroom = await invoke<Classroom>('classroom_create', {
      name,
      description: description ?? null,
      iconEmoji: iconEmoji ?? null,
    })
    classrooms.value = [...classrooms.value, classroom]
    return classroom
  } catch (e) {
    lastError.value = String(e)
    return null
  }
}

async function enterClassroom(id: string) {
  loading.value = true
  lastError.value = null
  try {
    await setupEventListeners()

    const [classroom, chans, mems] = await Promise.all([
      invoke<Classroom>('classroom_get', { classroomId: id }),
      invoke<ClassroomChannel[]>('classroom_list_channels', { classroomId: id }),
      invoke<ClassroomMember[]>('classroom_list_members', { classroomId: id }),
    ])

    currentClassroom.value = classroom
    channels.value = chans
    members.value = mems

    // Subscribe to P2P topics
    await invoke('classroom_subscribe', { classroomId: id }).catch(() => {})

    // Load active call if any
    const call = await invoke<ClassroomCall | null>('classroom_get_active_call', {
      classroomId: id,
    })
    activeCall.value = call

    // Load join requests if moderator/owner
    if (classroom.my_role === 'owner' || classroom.my_role === 'moderator') {
      joinRequests.value = await invoke<JoinRequest[]>('classroom_list_join_requests', {
        classroomId: id,
      })
    }
  } catch (e) {
    lastError.value = String(e)
  } finally {
    loading.value = false
  }
}

async function exitClassroom() {
  if (currentClassroom.value) {
    await invoke('classroom_unsubscribe', { classroomId: currentClassroom.value.id }).catch(
      () => {},
    )
  }
  currentClassroom.value = null
  channels.value = []
  members.value = []
  messages.value = {}
  joinRequests.value = []
  activeCall.value = null
  teardownEventListeners()
}

async function loadMessages(channelId: string, beforeId?: string) {
  try {
    const msgs = await invoke<ClassroomMessage[]>('classroom_get_messages', {
      channelId,
      beforeId: beforeId ?? null,
      limit: 50,
    })
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
    lastError.value = String(e)
  }
}

async function sendMessage(
  channelId: string,
  content: string,
): Promise<ClassroomMessage | null> {
  try {
    const msg = await invoke<ClassroomMessage>('classroom_send_message', {
      channelId,
      content,
    })
    const current = messages.value[channelId] ?? []
    if (!current.find((m) => m.id === msg.id)) {
      messages.value = {
        ...messages.value,
        [channelId]: [...current, msg],
      }
    }
    return msg
  } catch (e) {
    lastError.value = String(e)
    return null
  }
}

async function deleteMessage(messageId: string, channelId: string) {
  try {
    await invoke('classroom_delete_message', { messageId })
    messages.value = {
      ...messages.value,
      [channelId]: (messages.value[channelId] ?? []).map((m) =>
        m.id === messageId ? { ...m, deleted: true, content: '[deleted]' } : m,
      ),
    }
  } catch (e) {
    lastError.value = String(e)
  }
}

async function requestJoin(classroomId: string, message?: string) {
  try {
    await invoke('classroom_request_join', {
      classroomId,
      message: message ?? null,
    })
  } catch (e) {
    lastError.value = String(e)
    throw e
  }
}

async function approveRequest(classroomId: string, stakeAddress: string) {
  try {
    await invoke('classroom_approve_member', { classroomId, stakeAddress })
    joinRequests.value = joinRequests.value.filter(
      (r) => !(r.classroom_id === classroomId && r.stake_address === stakeAddress),
    )
  } catch (e) {
    lastError.value = String(e)
    throw e
  }
}

async function denyRequest(classroomId: string, stakeAddress: string) {
  try {
    await invoke('classroom_deny_member', { classroomId, stakeAddress })
    joinRequests.value = joinRequests.value.filter(
      (r) => !(r.classroom_id === classroomId && r.stake_address === stakeAddress),
    )
  } catch (e) {
    lastError.value = String(e)
    throw e
  }
}

async function leaveClassroom(classroomId: string) {
  try {
    await invoke('classroom_leave', { classroomId })
    classrooms.value = classrooms.value.filter((c) => c.id !== classroomId)
    if (currentClassroom.value?.id === classroomId) {
      await exitClassroom()
    }
  } catch (e) {
    lastError.value = String(e)
    throw e
  }
}

async function createChannel(
  classroomId: string,
  name: string,
  description?: string,
  channelType?: string,
): Promise<ClassroomChannel | null> {
  try {
    const channel = await invoke<ClassroomChannel>('classroom_create_channel', {
      classroomId,
      name,
      description: description ?? null,
      channelType: channelType ?? 'text',
    })
    channels.value = [...channels.value, channel].sort((a, b) => a.position - b.position)
    return channel
  } catch (e) {
    lastError.value = String(e)
    return null
  }
}

async function startCall(classroomId: string): Promise<ClassroomCall | null> {
  try {
    const call = await invoke<ClassroomCall>('classroom_start_call', {
      classroomId,
      channelId: null,
      displayName: null,
      cameraId: null,
      micId: null,
      speakerId: null,
    })
    activeCall.value = call
    return call
  } catch (e) {
    lastError.value = String(e)
    return null
  }
}

async function joinCall(callId: string): Promise<void> {
  try {
    await invoke('classroom_join_call', {
      callId,
      displayName: null,
      cameraId: null,
      micId: null,
      speakerId: null,
    })
  } catch (e) {
    lastError.value = String(e)
    throw e
  }
}

async function endCall(callId: string): Promise<void> {
  try {
    await invoke('classroom_end_call', { callId })
    activeCall.value = null
  } catch (e) {
    lastError.value = String(e)
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
