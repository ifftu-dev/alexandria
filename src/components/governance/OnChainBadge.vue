<script setup lang="ts">
defineProps<{
  txHash: string | null
  status?: 'local' | 'pending' | 'confirmed'
}>()

const explorerUrl = (hash: string) =>
  `https://preprod.cardanoscan.io/transaction/${hash}`
</script>

<template>
  <span
    class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[0.6rem] font-medium"
    :class="{
      'bg-muted/50 text-muted-foreground': !txHash && status !== 'pending',
      'bg-warning/10 text-warning': status === 'pending',
      'bg-success/10 text-success': txHash && status !== 'pending',
    }"
  >
    <span v-if="txHash" class="inline-block h-1.5 w-1.5 rounded-full bg-success" />
    <span v-else-if="status === 'pending'" class="inline-block h-1.5 w-1.5 rounded-full bg-warning animate-pulse" />
    <span v-else class="inline-block h-1.5 w-1.5 rounded-full bg-muted-foreground/40" />

    <template v-if="txHash">
      <a
        :href="explorerUrl(txHash)"
        target="_blank"
        rel="noopener"
        class="hover:underline"
        @click.stop
      >
        On-Chain
      </a>
    </template>
    <template v-else-if="status === 'pending'">
      Pending
    </template>
    <template v-else>
      Local Only
    </template>
  </span>
</template>
