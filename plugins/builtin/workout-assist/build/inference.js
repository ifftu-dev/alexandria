import * as tf from '@tensorflow/tfjs-core'
import '@tensorflow/tfjs-backend-webgl'
import '@tensorflow/tfjs-backend-cpu'
import { loadGraphModel } from '@tensorflow/tfjs-converter'
import graph from 'pose-graph'
import weights from 'pose-weights'

export async function openDetector() {
  if (!(await tf.setBackend('webgl'))) await tf.setBackend('cpu')
  const model = await loadGraphModel(
    tf.io.fromMemory({
      modelTopology: graph.modelTopology,
      weightSpecs: graph.weightsManifest[0].weights,
      weightData: weights.buffer,
    }),
  )
  const size = 192
  const canvas = document.createElement('canvas')
  canvas.width = canvas.height = size
  const ctx = canvas.getContext('2d', { willReadFrequently: true })
  return {
    async detect(video) {
      const scale = size / Math.max(video.videoWidth, video.videoHeight)
      const w = video.videoWidth * scale
      const h = video.videoHeight * scale
      const x = (size - w) / 2
      const y = (size - h) / 2
      ctx.fillStyle = 'black'
      ctx.fillRect(0, 0, size, size)
      ctx.drawImage(video, x, y, w, h)
      const output = tf.tidy(() =>
        model.execute(tf.expandDims(tf.browser.fromPixels(canvas), 0)),
      )
      try {
        const data = await output.data()
        // Common isotropic coordinates preserve angles on portrait and landscape cameras.
        return Array.from({ length: 17 }, (_, i) => [
          Math.round(data[3 * i + 1] * 1000),
          Math.round(data[3 * i] * 1000),
          Math.round(data[3 * i + 2] * 100),
        ])
      } finally {
        output.dispose()
      }
    },
    close() {
      model.dispose()
    },
  }
}
