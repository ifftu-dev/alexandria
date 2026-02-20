<script setup lang="ts">
interface Props {
  status: string
}

defineProps<Props>()

const statusVariants: Record<string, string> = {
  // Course status
  draft: 'badge-secondary',
  published: 'badge-success',
  archived: 'badge-warning',
  // Enrollment status
  active: 'badge-primary',
  completed: 'badge-success',
  dropped: 'badge-error',
  // Election phase
  nomination: 'badge-warning',
  voting: 'badge-primary',
  finalized: 'badge-success',
  cancelled: 'badge-error',
  // Proposal status
  approved: 'badge-success',
  rejected: 'badge-error',
  // Challenge status
  pending: 'badge-warning',
  reviewing: 'badge-primary',
  upheld: 'badge-success',
  expired: 'badge-secondary',
  // Snapshot status
  building: 'badge-warning',
  submitted: 'badge-primary',
  confirmed: 'badge-success',
  failed: 'badge-error',
  // P2P
  online: 'badge-success',
  offline: 'badge-secondary',
  // Generic
  ok: 'badge-success',
  error: 'badge-error',
}
</script>

<template>
  <span class="badge" :class="statusVariants[status.toLowerCase()] ?? 'badge-secondary'">
    {{ status }}
  </span>
</template>
