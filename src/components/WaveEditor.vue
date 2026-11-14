<script setup>
// 波形檢視＋片段選取＋播放選取。**只管一段已經解碼好的音訊**——怎麼拿到它、之後要拿它
// 做什麼是父層的事。
//
// 取樣與繪製在 waveform.js、四種拖曳的夾擠規則在 selection.js（純函式、各自有測試）。
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import Icon from './Icon.vue'
import { wholeMono } from '../audio/wav.js'
import { WAVE_HEIGHT, drawWaveform, pulseGlow, waveColors } from '../audio/waveform.js'
import { applyDrag, xToSeconds, zoneOf } from '../audio/selection.js'
import { fmtSeconds } from '../audio/speed.js'

const props = defineProps({
  /** 已解碼的 AudioBuffer。 */
  buffer: { type: Object, required: true },
  /** 播放用的 AudioContext 取得函式（由父層持有，才不會每個元件各開一個）。 */
  audioCtx: { type: Function, required: true },
  disabled: { type: Boolean, default: false },
})
const emit = defineEmits(['update'])

const wrap = ref(null)
const canvas = ref(null)

let mono = null // 全曲單聲道 Float32（畫波形用）
const duration = ref(0)
const sel = reactive({ start: 0, end: 0 })
const selDur = computed(() => Math.max(0, sel.end - sel.start))

// 播放
const playing = ref(false)
const playhead = ref(0)
let srcNode = null
let playRAF = null

// 把手 hover／載入提示動畫（純視覺，不影響選取邏輯）
const hoverZone = ref(null)
let pulseStart = 0
let pulseRAF = null

// 配色跟著亮／暗主題走；主題切換時重讀一次。
let colors = null
const darkQuery = window.matchMedia('(prefers-color-scheme: dark)')
const onTheme = () => { colors = null; draw() }

function emitUpdate() {
  emit('update', { start: sel.start, end: sel.end, duration: selDur.value, total: duration.value })
}

// 換了音訊：重畫、選取歸零回全曲、把手閃一下提示可拖曳。
watch(
  () => props.buffer,
  async (buf) => {
    stopPlayback()
    if (!buf) return
    mono = wholeMono(buf)
    duration.value = buf.duration
    sel.start = 0
    sel.end = buf.duration
    await nextTick()
    resize()
    draw()
    emitUpdate()
    startPulse()
  },
  { immediate: true },
)

// ---- 繪製 ----
function resize() {
  const c = canvas.value
  if (!c || !wrap.value) return
  const dpr = window.devicePixelRatio || 1
  const w = wrap.value.clientWidth
  c.width = w * dpr
  c.height = WAVE_HEIGHT * dpr
  c.style.width = w + 'px'
  c.style.height = WAVE_HEIGHT + 'px'
  c.getContext('2d').setTransform(dpr, 0, 0, dpr, 0, 0)
}

function draw() {
  const c = canvas.value
  if (!c) return
  colors ??= waveColors(c)
  drawWaveform(c.getContext('2d'), {
    mono,
    width: wrap.value.clientWidth,
    duration: duration.value,
    sel,
    // 拖曳中的那一端要高亮；沒在拖就看 hover（'new' 是從空白拉出，兩端都亮）
    activeStart: dragMode === 'start' || dragMode === 'new' || (!dragMode && hoverZone.value === 'start'),
    activeEnd: dragMode === 'end' || dragMode === 'new' || (!dragMode && hoverZone.value === 'end'),
    glow: pulseGlow(pulseStart, performance.now()),
    playhead: playing.value ? playhead.value : null,
    colors,
  })
}

function startPulse() {
  cancelAnimationFrame(pulseRAF)
  pulseStart = performance.now()
  const loop = () => {
    if (!pulseGlow(pulseStart, performance.now())) { stopPulse(); draw(); return }
    draw()
    pulseRAF = requestAnimationFrame(loop)
  }
  loop()
}
function stopPulse() {
  if (!pulseStart) return
  pulseStart = 0
  cancelAnimationFrame(pulseRAF)
  pulseRAF = null
}

// ---- 拖曳選取（規則在 selection.js）----
let dragMode = null
let dragArgs = {}

function evSec(e) {
  const r = canvas.value.getBoundingClientRect()
  return xToSeconds(e.clientX - r.left, r.width, duration.value)
}
function hoverAt(e) {
  const r = canvas.value.getBoundingClientRect()
  return zoneOf(evSec(e), sel, duration.value > 0 ? r.width / duration.value : 0)
}
function updateCursor() {
  const mode = dragMode || hoverZone.value
  canvas.value.style.cursor = mode === 'move'
    ? (dragMode ? 'grabbing' : 'grab')
    : ['start', 'end', 'new'].includes(mode) ? 'ew-resize' : 'crosshair'
}

function onDown(e) {
  if (!mono || props.disabled) return
  stopPlayback()
  stopPulse() // 已經開始互動，不用再閃提示動畫
  try { canvas.value.setPointerCapture(e.pointerId) } catch { /* pointer 尚未 active，忽略 */ }
  const s = evSec(e)
  const zone = hoverAt(e)
  if (zone === 'start' || zone === 'end') {
    dragMode = zone
    dragArgs = {}
  } else if (zone === 'move') {
    dragMode = 'move'
    dragArgs = { offset: s - sel.start }
  } else {
    dragMode = 'new'
    dragArgs = { anchor: s }
    sel.start = s
    sel.end = s
  }
  updateCursor()
  onMove(e)
}

function onMove(e) {
  if (!dragMode) {
    // 未在拖曳：只更新 hover 狀態（游標樣式＋把手高亮），不動選取範圍
    const z = hoverAt(e)
    if (z !== hoverZone.value) { hoverZone.value = z; updateCursor(); draw() }
    return
  }
  const next = applyDrag(dragMode, sel, evSec(e), duration.value, dragArgs)
  sel.start = next.start
  sel.end = next.end
  draw()
  emitUpdate()
}

function onUp(e) {
  if (dragMode === 'new' && sel.end - sel.start < 0.02) {
    // 只是點一下（沒拖出範圍）：當成「取消選取」回到全曲，而不是留下一段 0 秒的選取。
    sel.start = 0
    sel.end = duration.value
    emitUpdate()
  }
  dragMode = null
  try { canvas.value.releasePointerCapture(e.pointerId) } catch { /* ignore */ }
  updateCursor()
  draw()
}

function onLeave() {
  if (dragMode) return // 拖曳中（有 pointer capture）滑出畫布也不重置，等 onUp
  if (hoverZone.value !== null) { hoverZone.value = null; draw() }
}

function selectAll() {
  sel.start = 0
  sel.end = duration.value
  draw()
  emitUpdate()
}

// ---- 播放選取片段 ----
function playSelection() {
  if (!props.buffer) return
  stopPlayback()
  const ac = props.audioCtx()
  const src = ac.createBufferSource()
  src.buffer = props.buffer
  src.connect(ac.destination)
  const off = sel.start
  const t0 = ac.currentTime
  src.start(0, off, Math.max(0.02, sel.end - sel.start))
  srcNode = src
  playing.value = true
  playhead.value = off
  src.onended = () => { playing.value = false; cancelAnimationFrame(playRAF); draw() }
  const tick = () => {
    if (!playing.value) return
    playhead.value = Math.min(sel.end, off + (ac.currentTime - t0))
    draw()
    playRAF = requestAnimationFrame(tick)
  }
  tick()
}

function stopPlayback() {
  if (srcNode) {
    try { srcNode.onended = null; srcNode.stop() } catch { /* ignore */ }
    srcNode = null
  }
  playing.value = false
  cancelAnimationFrame(playRAF)
}

defineExpose({ stopPlayback })

let ro = null
onMounted(() => {
  ro = new ResizeObserver(() => { resize(); draw() })
  if (wrap.value) ro.observe(wrap.value)
  darkQuery.addEventListener('change', onTheme)
})
onBeforeUnmount(() => {
  stopPlayback()
  stopPulse()
  ro?.disconnect()
  darkQuery.removeEventListener('change', onTheme)
})
</script>

<template>
  <div class="editor">
    <div ref="wrap" class="wave-wrap">
      <canvas
        ref="canvas"
        class="wave"
        @pointerdown="onDown"
        @pointermove="onMove"
        @pointerup="onUp"
        @pointercancel="onUp"
        @pointerleave="onLeave"
      ></canvas>
    </div>

    <div class="controls">
      <button type="button" class="btn small" :disabled="disabled" @click="playing ? stopPlayback() : playSelection()">
        <Icon :name="playing ? 'stop' : 'play'" />{{ playing ? '停止' : '播放選取' }}
      </button>
      <button type="button" class="btn small" :disabled="disabled" @click="selectAll">全選</button>
      <span class="sel-info">
        選取 <strong>{{ fmtSeconds(selDur) }}</strong>
        <span class="muted">（{{ fmtSeconds(sel.start) }} → {{ fmtSeconds(sel.end) }}）</span>
      </span>
    </div>
    <p class="tip">在波形上拖曳選出要的片段；拖兩側把手調整範圍、拖中間整段移動。</p>
  </div>
</template>

<style scoped>
.wave-wrap { position: relative; width: 100%; background: var(--wave-bg); border: 1px solid var(--line); border-radius: 10px; overflow: hidden; }
.wave { display: block; width: 100%; height: 128px; cursor: crosshair; touch-action: none; }
.controls { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 10px; margin-top: 10px; }
.sel-info { color: var(--muted); font-size: 14px; font-variant-numeric: tabular-nums; }
.sel-info strong { color: var(--text); }
.muted { color: var(--muted); }
.tip { margin: 8px 0 0; color: var(--muted); font-size: 12px; }
</style>
