<template>
  <div class="h-screen flex bg-gray-950 text-white overflow-hidden">
    <!-- Left sidebar: channels + members -->
    <div class="w-60 flex-shrink-0 flex flex-col bg-gray-900 border-r border-gray-800">
      <!-- Classroom header -->
      <div class="px-4 py-3 border-b border-gray-800 flex items-center justify-between">
        <div class="flex items-center gap-2 min-w-0">
          <span class="text-lg">{{ currentClassroom?.icon_emoji ?? '🏫' }}</span>
          <span class="font-semibold text-white text-sm truncate">
            {{ currentClassroom?.name ?? 'Loading...' }}
          </span>
        </div>
        <RouterLink
          v-if="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
          :to="{ name: 'classroom-settings', params: { id: classroomId } }"
          class="text-gray-500 hover:text-white transition-colors text-sm"
          title="Settings"
        >
          ⚙
        </RouterLink>
      </div>

      <!-- Active call banner -->
      <div
        v-if="activeCall"
        class="mx-3 mt-3 p-2 bg-green-900/40 border border-green-700 rounded-lg"
      >
        <div class="text-xs text-green-300 font-medium mb-1">🎙 Voice call active</div>
        <button
          @click="handleJoinCall"
          class="w-full px-2 py-1 bg-green-700 hover:bg-green-600 text-white text-xs font-medium rounded transition-colors"
        >
          Join call
        </button>
      </div>

      <!-- Channels -->
      <div class="flex-1 overflow-y-auto py-3">
        <div class="px-3 mb-1 flex items-center justify-between">
          <span class="text-xs font-semibold text-gray-500 uppercase tracking-wider">Channels</span>
          <button
            v-if="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
            @click="showNewChannel = true"
            class="text-gray-500 hover:text-white transition-colors text-sm leading-none"
            title="Add channel"
          >
            +
          </button>
        </div>

        <button
          v-for="ch in channels"
          :key="ch.id"
          @click="selectChannel(ch.id)"
          :class="[
            'w-full flex items-center gap-2 px-3 py-1.5 rounded mx-1 text-sm transition-colors text-left',
            activeChannelId === ch.id
              ? 'bg-gray-700 text-white'
              : 'text-gray-400 hover:bg-gray-800 hover:text-white',
          ]"
        >
          <span class="text-gray-500 text-xs">#</span>
          <span class="truncate">{{ ch.name }}</span>
          <span
            v-if="ch.channel_type === 'announcement'"
            class="ml-auto text-xs text-yellow-500"
            title="Announcement channel"
          >📢</span>
        </button>
      </div>

      <!-- Members count -->
      <div class="px-4 py-3 border-t border-gray-800 text-xs text-gray-500">
        {{ members.length }} member{{ members.length !== 1 ? 's' : '' }}
        <RouterLink
          v-if="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
          :to="{ name: 'classroom-requests', params: { id: classroomId } }"
          class="ml-2 text-indigo-400 hover:text-indigo-300"
        >
          Requests{{ joinRequests.length > 0 ? ` (${joinRequests.length})` : '' }}
        </RouterLink>
      </div>
    </div>

    <!-- Main content: messages -->
    <div class="flex-1 flex flex-col min-w-0">
      <!-- Channel header -->
      <div class="px-4 py-3 border-b border-gray-800 flex items-center justify-between flex-shrink-0">
        <div class="flex items-center gap-2">
          <span class="text-gray-400">#</span>
          <span class="font-semibold">{{ activeChannel?.name ?? 'Select a channel' }}</span>
          <span v-if="activeChannel?.description" class="text-sm text-gray-500">
            · {{ activeChannel.description }}
          </span>
        </div>
        <div class="flex items-center gap-2">
          <button
            v-if="activeCall === null && currentClassroom"
            @click="handleStartCall"
            class="px-3 py-1 bg-green-800 hover:bg-green-700 text-white text-xs font-medium rounded transition-colors"
          >
            🎙 Start call
          </button>
        </div>
      </div>

      <!-- Messages -->
      <div ref="messageContainer" class="flex-1 overflow-y-auto px-4 py-4 space-y-1">
        <div v-if="!activeChannelId" class="flex items-center justify-center h-full text-gray-500 text-sm">
          Select a channel to start chatting
        </div>

        <div v-else-if="channelMessages.length === 0" class="flex items-center justify-center h-full">
          <div class="text-center text-gray-500">
            <div class="text-3xl mb-2">#</div>
            <p class="text-sm">No messages yet. Start the conversation!</p>
          </div>
        </div>

        <template v-else>
          <!-- Load more -->
          <button
            v-if="hasMore"
            @click="loadMore"
            class="w-full text-xs text-indigo-400 hover:text-indigo-300 py-2 transition-colors"
          >
            Load earlier messages
          </button>

          <div
            v-for="msg in channelMessages"
            :key="msg.id"
            class="flex gap-3 group hover:bg-gray-900/40 rounded px-2 py-1 -mx-2"
          >
            <!-- Avatar placeholder -->
            <div class="w-8 h-8 rounded-full bg-gray-700 flex items-center justify-center text-xs flex-shrink-0 mt-0.5">
              {{ (msg.sender_name ?? msg.sender_address).charAt(0).toUpperCase() }}
            </div>

            <div class="flex-1 min-w-0">
              <div class="flex items-baseline gap-2 mb-0.5">
                <span class="text-sm font-medium text-white">
                  {{ msg.sender_name ?? formatAddress(msg.sender_address) }}
                </span>
                <span class="text-xs text-gray-500">{{ formatTime(msg.sent_at) }}</span>
              </div>
              <p v-if="!msg.deleted" class="text-sm text-gray-200 break-words">{{ msg.content }}</p>
              <p v-else class="text-sm text-gray-500 italic">[deleted]</p>
            </div>

            <!-- Delete button (own messages or mod) -->
            <button
              v-if="!msg.deleted && canDeleteMessage(msg.sender_address)"
              @click="handleDeleteMessage(msg.id)"
              class="opacity-0 group-hover:opacity-100 text-gray-600 hover:text-red-400 text-xs transition-all"
            >
              🗑
            </button>
          </div>
        </template>
      </div>

      <!-- Message input -->
      <div v-if="activeChannelId" class="px-4 py-3 border-t border-gray-800 flex-shrink-0">
        <div class="flex gap-2 items-end">
          <textarea
            v-model="messageInput"
            @keydown.enter.exact.prevent="handleSend"
            :placeholder="`Message #${activeChannel?.name ?? 'channel'}`"
            rows="1"
            class="flex-1 px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 text-sm focus:outline-none focus:border-indigo-500 resize-none"
            :style="{ height: inputHeight + 'px' }"
            @input="adjustHeight"
          />
          <button
            @click="handleSend"
            :disabled="!messageInput.trim() || sending"
            class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium rounded-lg transition-colors flex-shrink-0"
          >
            Send
          </button>
        </div>
        <div class="text-xs text-gray-600 mt-1">Enter to send · Shift+Enter for newline</div>
      </div>
    </div>

    <!-- Right sidebar: members -->
    <div class="w-52 flex-shrink-0 bg-gray-900 border-l border-gray-800 overflow-y-auto">
      <div class="px-3 py-3">
        <div class="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Members</div>

        <!-- Owners -->
        <div v-if="owners.length > 0" class="mb-3">
          <div class="text-xs text-gray-600 mb-1">Owner</div>
          <div v-for="m in owners" :key="m.stake_address" class="flex items-center gap-2 py-1">
            <div class="w-6 h-6 rounded-full bg-yellow-800 flex items-center justify-center text-xs flex-shrink-0">
              {{ (m.display_name ?? m.stake_address).charAt(0).toUpperCase() }}
            </div>
            <span class="text-sm text-white truncate">
              {{ m.display_name ?? formatAddress(m.stake_address) }}
            </span>
          </div>
        </div>

        <!-- Moderators -->
        <div v-if="moderators.length > 0" class="mb-3">
          <div class="text-xs text-gray-600 mb-1">Moderators</div>
          <div v-for="m in moderators" :key="m.stake_address" class="flex items-center gap-2 py-1">
            <div class="w-6 h-6 rounded-full bg-indigo-800 flex items-center justify-center text-xs flex-shrink-0">
              {{ (m.display_name ?? m.stake_address).charAt(0).toUpperCase() }}
            </div>
            <span class="text-sm text-gray-300 truncate">
              {{ m.display_name ?? formatAddress(m.stake_address) }}
            </span>
          </div>
        </div>

        <!-- Members -->
        <div v-if="regularMembers.length > 0">
          <div class="text-xs text-gray-600 mb-1">Members — {{ regularMembers.length }}</div>
          <div v-for="m in regularMembers" :key="m.stake_address" class="flex items-center gap-2 py-1">
            <div class="w-6 h-6 rounded-full bg-gray-700 flex items-center justify-center text-xs flex-shrink-0">
              {{ (m.display_name ?? m.stake_address).charAt(0).toUpperCase() }}
            </div>
            <span class="text-sm text-gray-400 truncate">
              {{ m.display_name ?? formatAddress(m.stake_address) }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- New channel modal -->
    <Teleport to="body">
      <div
        v-if="showNewChannel"
        class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
        @click.self="showNewChannel = false"
      >
        <div class="bg-gray-900 rounded-xl border border-gray-700 w-full max-w-sm p-5">
          <h2 class="text-base font-semibold text-white mb-4">Create channel</h2>
          <input
            v-model="newChannelName"
            type="text"
            placeholder="channel-name"
            class="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 text-sm focus:outline-none focus:border-indigo-500 mb-3"
          />
          <select
            v-model="newChannelType"
            class="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm focus:outline-none focus:border-indigo-500 mb-4"
          >
            <option value="text">Text channel</option>
            <option value="announcement">Announcement channel</option>
          </select>
          <div class="flex gap-3">
            <button
              @click="showNewChannel = false"
              class="flex-1 px-3 py-2 bg-gray-800 hover:bg-gray-700 text-white text-sm rounded-lg transition-colors"
            >
              Cancel
            </button>
            <button
              @click="handleCreateChannel"
              :disabled="!newChannelName.trim()"
              class="flex-1 px-3 py-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-sm rounded-lg transition-colors"
            >
              Create
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useClassroom } from '@/composables/useClassroom'
import { useAuth } from '@/composables/useAuth'

const route = useRoute()
const classroomId = computed(() => route.params.id as string)

const {
  currentClassroom,
  channels,
  members,
  messages,
  joinRequests,
  activeCall,
  enterClassroom,
  exitClassroom,
  loadMessages,
  sendMessage,
  deleteMessage,
  createChannel,
  startCall,
  joinCall,
} = useClassroom()

const { walletInfo } = useAuth()

const activeChannelId = ref<string | null>(null)
const messageInput = ref('')
const inputHeight = ref(40)
const sending = ref(false)
const hasMore = ref(true)
const messageContainer = ref<HTMLElement | null>(null)
const showNewChannel = ref(false)
const newChannelName = ref('')
const newChannelType = ref<'text' | 'announcement'>('text')

const activeChannel = computed(() =>
  channels.value.find((c) => c.id === activeChannelId.value) ?? null,
)

const channelMessages = computed(() =>
  activeChannelId.value ? (messages.value[activeChannelId.value] ?? []) : [],
)

const owners = computed(() => members.value.filter((m) => m.role === 'owner'))
const moderators = computed(() => members.value.filter((m) => m.role === 'moderator'))
const regularMembers = computed(() => members.value.filter((m) => m.role === 'member'))

function canDeleteMessage(senderAddress: string): boolean {
  const myAddress = walletInfo.value?.stake_address
  if (!myAddress) return false
  if (senderAddress === myAddress) return true
  const myRole = currentClassroom.value?.my_role
  return myRole === 'owner' || myRole === 'moderator'
}

function selectChannel(id: string) {
  activeChannelId.value = id
  if (!(messages.value[id]?.length)) {
    loadMessages(id)
  }
}

function formatAddress(addr: string): string {
  return addr.length > 12 ? `${addr.slice(0, 8)}...${addr.slice(-4)}` : addr
}

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  } catch {
    return iso
  }
}

async function handleSend() {
  const content = messageInput.value.trim()
  if (!content || !activeChannelId.value || sending.value) return
  sending.value = true
  messageInput.value = ''
  inputHeight.value = 40
  try {
    await sendMessage(activeChannelId.value, content)
    await nextTick()
    scrollToBottom()
  } finally {
    sending.value = false
  }
}

async function handleDeleteMessage(msgId: string) {
  if (!activeChannelId.value) return
  await deleteMessage(msgId, activeChannelId.value)
}

async function loadMore() {
  if (!activeChannelId.value) return
  const firstId = channelMessages.value[0]?.id
  await loadMessages(activeChannelId.value, firstId)
  if (channelMessages.value[0]?.id === firstId) {
    hasMore.value = false
  }
}

function scrollToBottom() {
  if (messageContainer.value) {
    messageContainer.value.scrollTop = messageContainer.value.scrollHeight
  }
}

function adjustHeight(e: Event) {
  const el = e.target as HTMLTextAreaElement
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 120) + 'px'
  inputHeight.value = Math.min(el.scrollHeight, 120)
}

async function handleCreateChannel() {
  if (!newChannelName.value.trim()) return
  await createChannel(classroomId.value, newChannelName.value.trim(), undefined, newChannelType.value)
  newChannelName.value = ''
  newChannelType.value = 'text'
  showNewChannel.value = false
}

async function handleStartCall() {
  await startCall(classroomId.value)
}

async function handleJoinCall() {
  if (!activeCall.value) return
  await joinCall(activeCall.value.id)
}

// Auto-select first channel when channels load
watch(channels, (chans) => {
  const [firstChannel] = chans
  if (firstChannel && !activeChannelId.value) {
    selectChannel(firstChannel.id)
  }
})

// Scroll to bottom when new messages arrive
watch(channelMessages, () => {
  nextTick(() => scrollToBottom())
})

onMounted(async () => {
  await enterClassroom(classroomId.value)
})

onBeforeUnmount(() => {
  exitClassroom()
})
</script>
