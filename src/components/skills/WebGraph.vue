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
 *   interaction: layout positions and radii, the lit and named sets, line
 *   paths, name places, the camera and fit, roving focus, drag with its live
 *   run, pan, wheel and pinch. It takes the same inputs as this component's
 *   props, a root element whose descendants carry `data-skill`, and the
 *   layers it moves with the pan; it scales the drawing and places each
 *   dot's button for the zoom.
 * - `WebGraphDots` draws the dots and `WebGraphLines` the lines, as SVG
 *   groups in layout units scaled by the zoom; `WebGraphNames` is the names.
 *   None is a control: each dot's control is a transparent button.
 * - This component assembles them for the sidebar: one layer holding an SVG
 *   of lines and dots, one transparent button per dot (placed and sized by
 *   `useWebGraph`) and the names.
 * - The explorer (ticket 05) does not mount this component, because its node
 *   elements must stay the same DOM nodes across Web, Simple and Detailed so
 *   they can travel and keep focus. In Web its adapter (`useExplorerWeb`)
 *   calls `useWebGraph` with the explorer's map element as root and its
 *   canvas and names layer as layers; `useWebGraph` places and sizes the
 *   explorer's existing node elements as the dots' buttons. They render
 *   `WebGraphDot` in place of the Simple or Detailed skin, seen only while
 *   nodes travel between views, and bind the same attributes and handlers as
 *   the template below (`data-skill`, `tabindex` from `roving`,
 *   `aria-pressed`, click, pointerenter and pointerleave on the node; key,
 *   focus, pointer, pointerleave, click-capture and wheel handlers on the
 *   root). It draws `WebGraphLines` and `WebGraphDots` in its canvas and
 *   `WebGraphNames` in its names layer. `zoomBy` and `panBy` serve its zoom and pan
 *   buttons (`AtlasMapTools`); a null camera fits what is shown.
 * - The memoised Web graph and layout of a model come from `webLayoutEntry`
 *   (`@/utils/webGeometry`), computed once per model for every surface.
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
import { webLayoutEntry, type WebPoint } from '@/utils/webGeometry'
import { useWebGraph } from './useWebGraph'
import WebGraphDots from './WebGraphDots.vue'
import WebGraphLines from './WebGraphLines.vue'
import WebGraphNames from './WebGraphNames.vue'

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
const layer = ref<HTMLElement>()
const web = useWebGraph(() => ({
  model: props.model, graph: webLayoutEntry(props.model).graph, positions: props.positions, selectedId: props.selectedId, focusIds: props.focusIds, matchIds: props.matchIds,
  shownIds: props.shownIds, fitIds: props.fitIds, narrowed: props.narrowed, names: props.names, arrows: props.arrows, related: props.related,
  camera: props.camera, compact: props.compact, reducedMotion: props.reducedMotion,
}), {
  select: id => emit('select', id), active: id => emit('active', id),
  camera: camera => emit('update:camera', camera), drag: positions => emit('drag', positions),
}, root, () => [layer.value])
const { dots, dotLayers, selectedDot, names, lines, hoverId } = web
/** Accessible names, once per model and language: a pan or zoom only looks them up. */
const accessibleNames = computed(() => {
  void locale.value
  return new Map(props.model.nodes.map(node => [node.skill.id, present.accessibleName(node)]))
})
</script>

<template>
  <div ref="root" class="web-graph" :class="{ 'is-compact': compact, 'is-over-dot': hoverId !== null }" role="group" :aria-label="label" :aria-describedby="describedBy" @keydown="web.keyDown" @keyup="web.keyUp" @focusin="web.focusIn" @focusout="web.focusOut" @pointerdown="web.pointerDown" @pointermove="web.pointerMove" @pointerup="web.pointerUp" @pointercancel="web.pointerUp" @lostpointercapture="web.pointerUp" @pointerleave="web.pointerLeave" @click.capture="web.clickCapture" @wheel="web.wheel">
    <div ref="layer" class="web-layer">
      <svg class="web-canvas" width="1" height="1" aria-hidden="true"><WebGraphLines :lines="lines" :compact="compact" /><WebGraphDots :layers="dotLayers" :selected="selectedDot" :compact="compact" /></svg>
      <button v-for="dot in dots" :key="dot.id" class="web-node" :class="{ 'is-faded': dot.faded }" type="button" :data-skill="dot.id" :tabindex="dot.roving ? 0 : -1" :aria-label="accessibleNames.get(dot.id)" :aria-pressed="dot.selected" @click="web.dotClick(dot.id)" @pointerenter="web.dotEnter($event, dot.id)" @pointerleave="web.dotLeave($event)" />
      <WebGraphNames :names="names" :compact="compact" />
    </div>
  </div>
</template>

<style scoped>
.web-graph { position: relative; width: 100%; height: var(--web-graph-height, 520px); overflow: hidden; touch-action: none; user-select: none; cursor: grab; background: var(--app-background); }
.web-graph.is-compact { height: var(--web-graph-height, 200px); }
/* Moved by the pan (useWebGraph sets its transform); point 0, 0 of the layout sits at its origin. */
.web-layer { position: absolute; top: 0; left: 0; width: 0; height: 0; }
.web-canvas { position: absolute; top: 0; left: 0; display: block; overflow: visible; }
/* The dot's transparent button, placed and sized by useWebGraph (transform, width and height); the dot itself is drawn in the SVG. */
.web-node { position: absolute; top: 0; left: 0; padding: 0; margin: 0; border: 0; border-radius: 50%; background: transparent; cursor: pointer; }
.web-graph.is-over-dot { cursor: pointer; }
.web-node:focus-visible { outline: 3px solid var(--app-ring); outline-offset: 2px; }
</style>
