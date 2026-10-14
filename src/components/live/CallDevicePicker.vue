<script setup lang="ts">
// Camera, microphone and speaker for a call on this computer. Before joining
// all three apply when the call starts; during a call (`inCall`) the camera
// switches at once and the audio devices wait for the next join, since the
// audio pipeline is built with them.
import type { DeviceList } from '@/types'
import type { CallDeviceChoice } from '@/composables/useCallDevices'

defineProps<{
  devices: DeviceList | null
  inCall?: boolean
  disabled?: boolean
}>()

const choice = defineModel<CallDeviceChoice>({ required: true })
const emit = defineEmits<{ cameraChange: [camera: string | null] }>()

function setCamera(value: string) {
  const camera = value || null
  choice.value = { ...choice.value, camera }
  emit('cameraChange', camera)
}

function setAudio(key: 'mic' | 'speaker', value: string) {
  choice.value = { ...choice.value, [key]: value || null }
}
</script>

<template>
  <div class="space-y-3" data-testid="call-device-picker">
    <label class="block">
      <span class="label text-xs text-muted-foreground">{{ $t('tutoring.device.camera') }}</span>
      <select
        class="input"
        :value="choice.camera ?? ''"
        :disabled="disabled || !devices?.cameras.length"
        data-testid="device-camera"
        @change="setCamera(($event.target as HTMLSelectElement).value)"
      >
        <option value="">{{ $t('tutoring.device.automatic') }}</option>
        <option v-for="camera in devices?.cameras ?? []" :key="camera.index" :value="camera.index">{{ camera.name }}</option>
      </select>
      <span v-if="devices && !devices.cameras.length" class="mt-1 block text-xs text-muted-foreground">{{ $t('tutoring.device.noCameras') }}</span>
    </label>
    <label class="block">
      <span class="label text-xs text-muted-foreground">{{ $t('tutoring.device.microphone') }}</span>
      <select
        class="input"
        :value="choice.mic ?? ''"
        :disabled="disabled || !devices?.audio_inputs.length"
        data-testid="device-mic"
        @change="setAudio('mic', ($event.target as HTMLSelectElement).value)"
      >
        <option value="">{{ $t('tutoring.device.automatic') }}</option>
        <option v-for="device in devices?.audio_inputs ?? []" :key="device.id" :value="device.id">
          {{ device.name || device.id }}{{ device.is_default ? $t('tutoring.device.defaultSuffix') : '' }}
        </option>
      </select>
    </label>
    <label class="block">
      <span class="label text-xs text-muted-foreground">{{ $t('tutoring.device.speaker') }}</span>
      <select
        class="input"
        :value="choice.speaker ?? ''"
        :disabled="disabled || !devices?.audio_outputs.length"
        data-testid="device-speaker"
        @change="setAudio('speaker', ($event.target as HTMLSelectElement).value)"
      >
        <option value="">{{ $t('tutoring.device.automatic') }}</option>
        <option v-for="device in devices?.audio_outputs ?? []" :key="device.id" :value="device.id">
          {{ device.name || device.id }}{{ device.is_default ? $t('tutoring.device.defaultSuffix') : '' }}
        </option>
      </select>
    </label>
    <p v-if="inCall" class="text-xs text-muted-foreground">{{ $t('tutoring.device.audioNextJoin') }}</p>
  </div>
</template>
