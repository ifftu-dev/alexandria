<script setup lang="ts">
import AppModal from './AppModal.vue'
import AppButton from './AppButton.vue'

interface Props {
  open: boolean
  title: string
  message?: string
  confirmLabel?: string
  confirmVariant?: 'primary' | 'danger' | 'governance'
  loading?: boolean
}

withDefaults(defineProps<Props>(), {
  message: '',
  confirmLabel: 'Confirm',
  confirmVariant: 'primary',
  loading: false,
})

defineEmits<{ confirm: []; cancel: [] }>()
</script>

<template>
  <AppModal :open="open" :title="title" max-width="28rem" @close="$emit('cancel')">
    <p v-if="message" class="text-sm text-[rgb(var(--color-muted-foreground))] mb-4">
      {{ message }}
    </p>
    <slot />
    <template #footer>
      <div class="flex justify-end gap-2">
        <AppButton variant="ghost" :disabled="loading" @click="$emit('cancel')">
          Cancel
        </AppButton>
        <AppButton :variant="confirmVariant" :loading="loading" @click="$emit('confirm')">
          {{ confirmLabel }}
        </AppButton>
      </div>
    </template>
  </AppModal>
</template>
