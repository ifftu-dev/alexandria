<script setup lang="ts">
import { ref, watch, onMounted } from 'vue'
import QRCode from 'qrcode'

const props = defineProps<{
  value: string
  size?: number
}>()

const canvasRef = ref<HTMLCanvasElement | null>(null)
const error = ref<string | null>(null)

async function render() {
  if (!canvasRef.value || !props.value) return
  error.value = null
  try {
    await QRCode.toCanvas(canvasRef.value, props.value, {
      width: props.size ?? 280,
      margin: 2,
      errorCorrectionLevel: 'M',
      color: {
        dark: '#000000',
        light: '#ffffff',
      },
    })
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

onMounted(render)
watch(() => [props.value, props.size], render)
</script>

<template>
  <div class="flex flex-col items-center">
    <div class="rounded-lg bg-white p-3">
      <canvas ref="canvasRef" />
    </div>
    <p v-if="error" class="mt-2 text-xs text-destructive">{{ error }}</p>
  </div>
</template>
