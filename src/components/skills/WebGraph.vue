<script setup lang="ts">
/**
 * The Web graph: every skill as a dot laid out by attraction and repulsion,
 * prerequisite lines and dotted relation lines, names for what is lit, hover
 * and keyboard emphasis, one Tab stop with arrow-key roving, dot drag with a
 * live force run, wheel and pinch zoom and background pan. It owns no tools:
 * search, narrowing, the toggles, zoom buttons and Reset belong to the owner,
 * which passes their state in and stores what the graph emits.
 *
 * How the work is split, so the sidebar and the explorer cannot drift apart:
 * - `useWebGraph` (beside this file) holds all geometry, emphasis and
 *   interaction: screen positions and dot sizes, the lit and named sets,
 *   line paths, the camera and fit, roving focus, drag with its live run,
 *   pan, wheel and pinch. It takes the same inputs as this component's props
 *   and a root element whose descendants carry `data-skill`.
 * - `WebGraphDot` is the Web skin of one skill (dot and name); `WebGraphLines`
 *   is the lines as an SVG group. Neither is a control.
 * - This component assembles them for the sidebar: an SVG of lines under one
 *   button per dot.
 * - The explorer (ticket 05) does not mount this component, because its node
 *   elements must stay the same DOM nodes across Web, Simple and Detailed so
 *   they can travel and keep focus. In Web it calls `useWebGraph` with its
 *   map element as root, places its existing node elements at each dot's
 *   `x`, `y` and `hit` (screen pixels), renders `WebGraphDot` inside them in
 *   place of the Simple or Detailed skin, binds the same attributes and
 *   handlers as the template below (`data-skill`, `tabindex` from `roving`,
 *   `aria-pressed`, click, pointerenter and pointerleave on the node; key,
 *   focus, pointer, click-capture and wheel handlers on the root), and draws
 *   `WebGraphLines` in its canvas. `zoomBy` and `panBy` serve its zoom and pan
 *   buttons; a null camera fits what is shown.
 *
 * Positions and camera are controlled: the owner passes the shared dragged
 * positions (`useSkillAtlas().webPositions`) and stores every `drag` event
 * (`setWebPositions`); it stores `update:camera` in its own Web camera. During
 * a live drag the graph draws its working positions, so an owner that stores
 * nothing sees the dot spring back when the run stops.
 */
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { AtlasModel } from '@/utils/skillAtlas'
import type { AtlasCamera } from '@/utils/skillAtlasView'
import { createAtlasPresenter } from './atlasPresentation'
import type { WebPoint } from './webGeometry'
import { useWebGraph } from './useWebGraph'
import WebGraphDot from './WebGraphDot.vue'
import WebGraphLines from './WebGraphLines.vue'

const props = withDefaults(defineProps<{
  model: AtlasModel
  positions: ReadonlyMap<string, WebPoint>
  label: string
  describedBy?: string
  selectedId?: string | null
  focusIds?: ReadonlySet<string> | null
  matchIds?: ReadonlySet<string> | null
  shownIds?: ReadonlySet<string> | null
  fitIds?: ReadonlySet<string> | null
  narrowed?: boolean
  names?: boolean
  arrows?: boolean
  related?: boolean
  camera?: AtlasCamera | null
  compact?: boolean
  reducedMotion?: boolean
}>(), { selectedId: null, focusIds: null, matchIds: null, shownIds: null, fitIds: null, camera: null, related: true })
const emit = defineEmits<{
  select: [id: string | null]
  active: [id: string | null]
  'update:camera': [camera: AtlasCamera]
  drag: [positions: ReadonlyMap<string, WebPoint>]
}>()
const { t, locale } = useI18n()
const present = createAtlasPresenter(t)
const root = ref<HTMLElement>()
const web = useWebGraph(() => ({
  model: props.model, positions: props.positions, selectedId: props.selectedId, focusIds: props.focusIds, matchIds: props.matchIds,
  shownIds: props.shownIds, fitIds: props.fitIds, narrowed: props.narrowed, names: props.names, arrows: props.arrows, related: props.related,
  camera: props.camera, compact: props.compact, reducedMotion: props.reducedMotion,
}), {
  select: id => emit('select', id), active: id => emit('active', id),
  camera: camera => emit('update:camera', camera), drag: positions => emit('drag', positions),
}, root)
const { dots, lines, size } = web
/** Accessible names, once per model and language: a pan or zoom only looks them up. */
const accessibleNames = computed(() => {
  void locale.value
  return new Map(props.model.nodes.map(node => [node.skill.id, present.accessibleName(node)]))
})
defineExpose({ focus: web.focus, zoomBy: web.zoomBy, panBy: web.panBy })
</script>

<template>
  <div ref="root" class="web-graph" :class="{ 'is-compact': compact }" role="group" :aria-label="label" :aria-describedby="describedBy" @keydown="web.keyDown" @keyup="web.keyUp" @focusin="web.focusIn" @focusout="web.focusOut" @pointerdown="web.pointerDown" @pointermove="web.pointerMove" @pointerup="web.pointerUp" @pointercancel="web.pointerUp" @lostpointercapture="web.pointerUp" @click.capture="web.clickCapture" @wheel="web.wheel">
    <svg class="web-canvas" :width="size.width" :height="size.height" :viewBox="`0 0 ${size.width} ${size.height}`" aria-hidden="true"><WebGraphLines :lines="lines" /></svg>
    <button v-for="dot in dots" :key="dot.id" class="web-node" type="button" :data-skill="dot.id" :tabindex="dot.roving ? 0 : -1" :aria-label="accessibleNames.get(dot.id)" :aria-pressed="dot.selected" :style="{ '--field': dot.color, '--hit': `${dot.hit}px`, transform: `translate(${dot.x - dot.hit / 2}px, ${dot.y - dot.hit / 2}px)` }" @click="web.dotClick(dot.id)" @pointerenter="web.hover(dot.id)" @pointerleave="web.hover(null)">
      <WebGraphDot :state="dot.state" :size="dot.size" :name="dot.node.skill.name" :labelled="dot.labelled" :selected="dot.selected" :faded="dot.faded" :compact="compact" :place="dot.namePlace" />
    </button>
  </div>
</template>

<style scoped>
.web-graph { position: relative; width: 100%; height: var(--web-graph-height, 520px); overflow: hidden; touch-action: none; user-select: none; cursor: grab; background: var(--app-background); }
.web-graph.is-compact { height: var(--web-graph-height, 200px); }
.web-canvas { position: absolute; top: 0; left: 0; display: block; overflow: visible; }
.web-node { position: absolute; top: 0; left: 0; display: grid; place-items: center; width: var(--hit); height: var(--hit); padding: 0; margin: 0; border: 0; border-radius: 50%; background: transparent; cursor: pointer; }
.web-node:focus-visible { outline: 3px solid var(--app-ring); outline-offset: 2px; }
</style>
