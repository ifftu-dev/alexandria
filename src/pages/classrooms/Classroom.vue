<template>
  <div class="h-full flex flex-col md:flex-row overflow-hidden">
    <!-- ── Mobile header ─────────────────────────────────────── -->
    <div class="md:hidden flex items-center gap-2 px-3 py-2.5 border-b border-border bg-card flex-shrink-0 safe-area-top safe-area-lr">
      <button @click="showChannelDrawer = true" class="p-1.5 rounded-md text-muted-foreground hover:bg-muted/50 transition-colors">
        <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
          <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5" />
        </svg>
      </button>
      <div class="flex-1 min-w-0">
        <div class="text-sm font-semibold text-foreground truncate">
          {{ currentClassroom?.name ?? 'Loading...' }}
        </div>
        <div v-if="activeChannel" class="text-xs text-muted-foreground truncate">
          # {{ activeChannel.name }}
        </div>
      </div>
      <button @click="showMembersDrawer = true" class="p-1.5 rounded-md text-muted-foreground hover:bg-muted/50 transition-colors">
        <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
          <path stroke-linecap="round" stroke-linejoin="round" d="M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.375a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zm8.25 2.25a2.625 2.625 0 11-5.25 0 2.625 2.625 0 015.25 0z" />
        </svg>
      </button>
    </div>

    <!-- ── Left sidebar: channels (desktop) ──────────────────── -->
    <div class="hidden md:flex w-60 flex-shrink-0 flex-col bg-card border-r border-border">
      <!-- Classroom header -->
      <div class="px-4 py-3 border-b border-border flex items-center justify-between">
        <div class="flex items-center gap-2 min-w-0">
          <span class="text-lg">{{ currentClassroom?.icon_emoji ?? '🏫' }}</span>
          <span class="font-semibold text-foreground text-sm truncate">
            {{ currentClassroom?.name ?? 'Loading...' }}
          </span>
        </div>
        <RouterLink
          v-if="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
          :to="{ name: 'classroom-settings', params: { id: classroomId } }"
          class="text-muted-foreground hover:text-foreground transition-colors text-sm"
          title="Settings"
        >
          <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 010 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 010-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28z" />
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
        </RouterLink>
      </div>

      <!-- Active call banner -->
      <div
        v-if="activeCall"
        class="mx-3 mt-3 p-2 bg-success/10 border border-success/30 rounded-lg"
      >
        <div class="text-xs text-success font-medium mb-1">🎙 Voice call active</div>
        <AppButton variant="primary" size="xs" class="w-full" @click="handleJoinCall">
          Join call
        </AppButton>
      </div>

      <ChannelList
        :channels="channels"
        :active-channel-id="activeChannelId ?? undefined"
        :can-manage="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
        @select="selectChannel"
        @add="showNewChannel = true"
      />

      <!-- Members count -->
      <div class="px-4 py-3 border-t border-border text-xs text-muted-foreground">
        {{ members.length }} member{{ members.length !== 1 ? 's' : '' }}
        <RouterLink
          v-if="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
          :to="{ name: 'classroom-requests', params: { id: classroomId } }"
          class="ml-2 text-primary hover:text-primary/80"
        >
          Requests{{ joinRequests.length > 0 ? ` (${joinRequests.length})` : '' }}
        </RouterLink>
      </div>
    </div>

    <!-- ── Main content: messages ─────────────────────────────── -->
    <div class="flex-1 flex flex-col min-w-0">
      <!-- Channel header (desktop) -->
      <div class="hidden md:flex px-4 py-3 border-b border-border items-center justify-between flex-shrink-0">
        <div class="flex items-center gap-2">
          <span class="text-muted-foreground">#</span>
          <span class="font-semibold text-foreground">{{ activeChannel?.name ?? 'Select a channel' }}</span>
          <span v-if="activeChannel?.description" class="text-sm text-muted-foreground">
            · {{ activeChannel.description }}
          </span>
        </div>
        <div class="flex items-center gap-2">
          <AppButton
            v-if="activeCall === null && currentClassroom"
            variant="outline"
            size="xs"
            @click="handleStartCall"
          >
            🎙 Start call
          </AppButton>
        </div>
      </div>

      <!-- Messages -->
      <div ref="messageContainer" class="flex-1 overflow-y-auto px-4 py-4 space-y-1">
        <div v-if="!activeChannelId" class="flex items-center justify-center h-full text-muted-foreground text-sm">
          Select a channel to start chatting
        </div>

        <div v-else-if="channelMessages.length === 0" class="flex items-center justify-center h-full">
          <div class="text-center">
            <div class="w-12 h-12 rounded-2xl bg-primary/10 flex items-center justify-center text-xl mx-auto mb-3">#</div>
            <p class="text-sm text-muted-foreground">No messages yet. Start the conversation!</p>
          </div>
        </div>

        <template v-else>
          <!-- Load more -->
          <button
            v-if="hasMore"
            @click="loadMore"
            class="w-full text-xs text-primary hover:text-primary/80 py-2 transition-colors"
          >
            Load earlier messages
          </button>

          <div
            v-for="msg in channelMessages"
            :key="msg.id"
            class="flex gap-3 group hover:bg-muted/50 rounded px-2 py-1 -mx-2"
          >
            <!-- Avatar -->
            <div class="w-8 h-8 rounded-full bg-muted flex items-center justify-center text-xs text-muted-foreground flex-shrink-0 mt-0.5">
              {{ (msg.sender_name ?? msg.sender_address).charAt(0).toUpperCase() }}
            </div>

            <div class="flex-1 min-w-0">
              <div class="flex items-baseline gap-2 mb-0.5">
                <span class="text-sm font-medium text-foreground">
                  {{ msg.sender_name ?? formatAddress(msg.sender_address) }}
                </span>
                <span class="text-xs text-muted-foreground">{{ formatTime(msg.sent_at) }}</span>
              </div>
              <p v-if="!msg.deleted" class="text-sm text-foreground/90 break-words">{{ msg.content }}</p>
              <p v-else class="text-sm text-muted-foreground italic">[deleted]</p>
            </div>

            <!-- Delete button -->
            <button
              v-if="!msg.deleted && canDeleteMessage(msg.sender_address)"
              @click="handleDeleteMessage(msg.id)"
              class="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive text-xs transition-all"
            >
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                <path stroke-linecap="round" stroke-linejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
              </svg>
            </button>
          </div>
        </template>
      </div>

      <!-- Message input -->
      <div v-if="activeChannelId" class="px-3 py-3 border-t border-border flex-shrink-0 safe-area-bottom safe-area-lr">
        <div class="flex gap-2 items-end">
          <textarea
            v-model="messageInput"
            @keydown.enter.exact.prevent="handleSend"
            :placeholder="`Message #${activeChannel?.name ?? 'channel'}`"
            rows="1"
            class="flex-1 px-3 py-2 bg-muted border border-border rounded-lg text-foreground placeholder-muted-foreground text-sm focus:outline-none focus:border-primary resize-none"
            :style="{ height: inputHeight + 'px' }"
            @input="adjustHeight"
          />
          <AppButton
            variant="primary"
            size="sm"
            :disabled="!messageInput.trim() || sending"
            @click="handleSend"
          >
            Send
          </AppButton>
        </div>
        <div class="hidden md:block text-xs text-muted-foreground/60 mt-1">Enter to send · Shift+Enter for newline</div>
      </div>
    </div>

    <!-- ── Right sidebar: members (desktop) ──────────────────── -->
    <div class="hidden md:block w-52 flex-shrink-0 bg-card border-l border-border overflow-y-auto">
      <MemberList :members="members" />
    </div>

    <!-- ── Mobile channel drawer ─────────────────────────────── -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-200 ease-out"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition duration-150 ease-in"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div v-if="showChannelDrawer" class="md:hidden fixed inset-0 z-40" @click.self="showChannelDrawer = false">
          <div class="absolute inset-0 bg-black/50" @click="showChannelDrawer = false" />
          <Transition
            enter-active-class="transition duration-200 ease-out"
            enter-from-class="-translate-x-full"
            enter-to-class="translate-x-0"
            leave-active-class="transition duration-150 ease-in"
            leave-from-class="translate-x-0"
            leave-to-class="-translate-x-full"
            appear
          >
            <div class="absolute inset-y-0 left-0 w-72 bg-card border-r border-border flex flex-col safe-area-top safe-area-bottom safe-area-lr">
              <!-- Header -->
              <div class="px-4 py-3 border-b border-border flex items-center justify-between">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="text-lg">{{ currentClassroom?.icon_emoji ?? '🏫' }}</span>
                  <span class="font-semibold text-foreground text-sm truncate">
                    {{ currentClassroom?.name ?? 'Loading...' }}
                  </span>
                </div>
                <button @click="showChannelDrawer = false" class="p-1 text-muted-foreground hover:text-foreground">
                  <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>

              <!-- Active call banner (mobile) -->
              <div
                v-if="activeCall"
                class="mx-3 mt-3 p-2 bg-success/10 border border-success/30 rounded-lg"
              >
                <div class="text-xs text-success font-medium mb-1">🎙 Voice call active</div>
                <AppButton variant="primary" size="xs" class="w-full" @click="handleJoinCall; showChannelDrawer = false">
                  Join call
                </AppButton>
              </div>

              <ChannelList
                :channels="channels"
                :active-channel-id="activeChannelId ?? undefined"
                :can-manage="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
                @select="(id: string) => { selectChannel(id); showChannelDrawer = false }"
                @add="showNewChannel = true; showChannelDrawer = false"
              />

              <!-- Footer -->
              <div class="px-4 py-3 border-t border-border text-xs text-muted-foreground">
                {{ members.length }} member{{ members.length !== 1 ? 's' : '' }}
                <RouterLink
                  v-if="currentClassroom?.my_role === 'owner' || currentClassroom?.my_role === 'moderator'"
                  :to="{ name: 'classroom-settings', params: { id: classroomId } }"
                  class="ml-2 text-primary hover:text-primary/80"
                  @click="showChannelDrawer = false"
                >
                  Settings
                </RouterLink>
              </div>
            </div>
          </Transition>
        </div>
      </Transition>
    </Teleport>

    <!-- ── Mobile members drawer ─────────────────────────────── -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-200 ease-out"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition duration-150 ease-in"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div v-if="showMembersDrawer" class="md:hidden fixed inset-0 z-40" @click.self="showMembersDrawer = false">
          <div class="absolute inset-0 bg-black/50" @click="showMembersDrawer = false" />
          <Transition
            enter-active-class="transition duration-200 ease-out"
            enter-from-class="translate-x-full"
            enter-to-class="translate-x-0"
            leave-active-class="transition duration-150 ease-in"
            leave-from-class="translate-x-0"
            leave-to-class="translate-x-full"
            appear
          >
            <div class="absolute inset-y-0 right-0 w-72 bg-card border-l border-border safe-area-top safe-area-bottom safe-area-lr overflow-y-auto">
              <div class="px-4 py-3 border-b border-border flex items-center justify-between">
                <span class="font-semibold text-foreground text-sm">Members</span>
                <button @click="showMembersDrawer = false" class="p-1 text-muted-foreground hover:text-foreground">
                  <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>
              <MemberList :members="members" />
            </div>
          </Transition>
        </div>
      </Transition>
    </Teleport>

    <!-- ── New channel modal ─────────────────────────────────── -->
    <AppModal :open="showNewChannel" @close="showNewChannel = false" title="Create channel">
      <div class="space-y-3">
        <input
          v-model="newChannelName"
          type="text"
          placeholder="channel-name"
          class="w-full px-3 py-2 bg-muted border border-border rounded-lg text-foreground placeholder-muted-foreground text-sm focus:outline-none focus:border-primary"
        />
        <select
          v-model="newChannelType"
          class="w-full px-3 py-2 bg-muted border border-border rounded-lg text-foreground text-sm focus:outline-none focus:border-primary"
        >
          <option value="text">Text channel</option>
          <option value="announcement">Announcement channel</option>
        </select>
      </div>
      <template #footer>
        <div class="flex gap-3">
          <AppButton variant="secondary" class="flex-1" @click="showNewChannel = false">
            Cancel
          </AppButton>
          <AppButton
            variant="primary"
            class="flex-1"
            :disabled="!newChannelName.trim()"
            @click="handleCreateChannel"
          >
            Create
          </AppButton>
        </div>
      </template>
    </AppModal>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch, defineComponent, h, type PropType } from 'vue'
import { useRoute } from 'vue-router'
import { useClassroom } from '@/composables/useClassroom'
import { useAuth } from '@/composables/useAuth'
import AppButton from '@/components/ui/AppButton.vue'
import AppModal from '@/components/ui/AppModal.vue'

// ── Inline sub-components ────────────────────────────────────────────

/** Channel list — shared between desktop sidebar and mobile drawer */
const ChannelList = defineComponent({
  props: {
    channels: { type: Array as PropType<readonly any[]>, required: true },
    activeChannelId: { type: String, default: undefined },
    canManage: { type: Boolean, default: false },
  },
  emits: ['select', 'add'],
  setup(props, { emit }) {
    return () =>
      h('div', { class: 'flex-1 overflow-y-auto py-3' }, [
        h('div', { class: 'px-3 mb-1 flex items-center justify-between' }, [
          h('span', { class: 'text-xs font-semibold text-muted-foreground uppercase tracking-wider' }, 'Channels'),
          props.canManage
            ? h(
                'button',
                {
                  onClick: () => emit('add'),
                  class: 'text-muted-foreground hover:text-foreground transition-colors text-sm leading-none',
                  title: 'Add channel',
                },
                '+',
              )
            : null,
        ]),
        ...(props.channels as any[]).map((ch: any) =>
          h(
            'button',
            {
              key: ch.id,
              onClick: () => emit('select', ch.id),
              class: [
                'w-full flex items-center gap-2 px-3 py-1.5 rounded mx-1 text-sm transition-colors text-left',
                props.activeChannelId === ch.id
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-muted-foreground hover:bg-muted/50 hover:text-foreground',
              ],
            },
            [
              h('span', { class: 'text-muted-foreground/60 text-xs' }, '#'),
              h('span', { class: 'truncate' }, ch.name),
              ch.channel_type === 'announcement'
                ? h('span', { class: 'ml-auto text-xs text-warning', title: 'Announcement channel' }, '📢')
                : null,
            ],
          ),
        ),
      ])
  },
})

/** Member list — shared between desktop sidebar and mobile drawer */
const MemberList = defineComponent({
  props: {
    members: { type: Array as PropType<readonly any[]>, required: true },
  },
  setup(props) {
    function formatAddr(addr: string): string {
      return addr.length > 12 ? `${addr.slice(0, 8)}...${addr.slice(-4)}` : addr
    }
    function roleSection(label: string, filtered: any[], avatarClass: string) {
      if (filtered.length === 0) return null
      return h('div', { class: 'mb-3' }, [
        h('div', { class: 'text-xs text-muted-foreground/60 mb-1' }, label),
        ...filtered.map((m: any) =>
          h('div', { key: m.stake_address, class: 'flex items-center gap-2 py-1' }, [
            h(
              'div',
              { class: `w-6 h-6 rounded-full flex items-center justify-center text-xs flex-shrink-0 ${avatarClass}` },
              (m.display_name ?? m.stake_address).charAt(0).toUpperCase(),
            ),
            h(
              'span',
              { class: 'text-sm text-foreground/80 truncate' },
              m.display_name ?? formatAddr(m.stake_address),
            ),
          ]),
        ),
      ])
    }
    return () => {
      const all = props.members as any[]
      return h('div', { class: 'px-3 py-3' }, [
        h('div', { class: 'text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-2' }, 'Members'),
        roleSection('Owner', all.filter((m) => m.role === 'owner'), 'bg-warning/20 text-warning'),
        roleSection('Moderators', all.filter((m) => m.role === 'moderator'), 'bg-primary/20 text-primary'),
        roleSection(
          `Members — ${all.filter((m) => m.role === 'member').length}`,
          all.filter((m) => m.role === 'member'),
          'bg-muted text-muted-foreground',
        ),
      ])
    }
  },
})

// ── Main setup ────────────────────────────────────────────────────────

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
const showChannelDrawer = ref(false)
const showMembersDrawer = ref(false)

const activeChannel = computed(() =>
  channels.value.find((c) => c.id === activeChannelId.value) ?? null,
)

const channelMessages = computed(() =>
  activeChannelId.value ? (messages.value[activeChannelId.value] ?? []) : [],
)

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
