<script setup>
// 音檔變速：挑一段、調快慢（音高不變）、輸出 WAV。全部在瀏覽器裡做——
//   * 解碼：瀏覽器內建的 decodeAudioData（mp3／m4a／flac… 播得了的都解得開）
//   * 變速：rubberband R2 的 Rust 移植編成 WASM，在 Web Worker 裡跑（見 stretch/）
//   * 串接：ZIP 編輯器可以把音檔傳過來，每做出一個結果就自動傳回去；也能手動打包傳過去（見 bridge.js）
import { computed, markRaw, onBeforeUnmount, onMounted, ref } from 'vue'
import Icon from './components/Icon.vue'
import WaveEditor from './components/WaveEditor.vue'
import { bridgeSupported, listen, sendResult, sendToEditor, toolLabel } from './bridge.js'
import { makeZip } from './zip.js'
import { fmtBytes, saveBlob, timeStamp, uniqueName } from './util.js'
import { encodeWav, extractChannels } from './audio/wav.js'
import {
  MAX_SPEED, MAX_STRETCH_SECONDS, MIN_SPEED, SPEED_PRESETS,
  clampSpeed, fmtSeconds, isUnitSpeed, outputSeconds, suggestName,
} from './audio/speed.js'
import { stretchChannels } from './stretch/index.js'

// ---------- toast ----------
const toastMsg = ref('')
let toastTimer
function toast(msg) {
  toastMsg.value = msg
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => { toastMsg.value = '' }, 3600)
}

let ctx = null
function audioCtx() {
  if (!ctx) ctx = new (window.AudioContext || window.webkitAudioContext)()
  if (ctx.state === 'suspended') ctx.resume?.()
  return ctx
}

// ---------- 來源 ----------
// File 與 AudioBuffer 一律 markRaw：交給 Vue 代理的話，getChannelData() 之類的原生方法
// 會因為 `this` 是 Proxy 而丟 Illegal invocation。
const sources = ref([]) // { id, file, buffer, error, loading }
const currentId = ref(null)
const current = computed(() => sources.value.find((s) => s.id === currentId.value) || null)
const editor = ref(null)
const fileInput = ref(null)

const AUDIO_EXT = /\.(mp3|m4a|m4b|aac|wav|wave|ogg|oga|opus|flac|webm|weba|mp4|m4v|mov|caf|aiff?)$/i
const isAudio = (f) => /^(audio|video)\//.test(f.type) || AUDIO_EXT.test(f.name)

// origins[i]：files[i] 從哪個工具來（見 bridge.js），用它做出的結果會自動傳回去。
function addFiles(list, from, origins = []) {
  const files = [...list]
  const picked = files.map((f, i) => [f, origins[i] ?? null]).filter(([f]) => isAudio(f))
  const audio = picked.map(([f]) => f)
  if (!audio.length) {
    if (files.length) toast('沒有可以處理的音檔')
    return
  }
  const added = picked.map(([f, origin]) => ({ id: crypto.randomUUID(), file: markRaw(f), origin: origin && markRaw(origin), buffer: null, error: '', loading: null }))
  sources.value.push(...added)
  if (!current.value?.buffer || from) select(added[0].id)
  const skipped = files.length - audio.length
  const who = from ? `從${toolLabel(from)}收到` : '已加入'
  const back = picked.find(([, o]) => o)?.[1]
  toast(`${who} ${audio.length} 個音檔${skipped ? `（略過 ${skipped} 個不是音檔的）` : ''}${back ? `，做好的結果會自動傳回${toolLabel(back.app)}` : ''}`)
}

/** 解碼（只做一次）；失敗時把原因記在來源上，回 null。 */
function ensureDecoded(s) {
  if (s.buffer || s.error) return Promise.resolve(s.buffer)
  s.loading ??= (async () => {
    try {
      s.buffer = markRaw(await audioCtx().decodeAudioData(await s.file.arrayBuffer()))
    } catch {
      s.error = '無法解碼這個檔案：瀏覽器不支援這種格式，或檔案已損毀'
    } finally {
      s.loading = null
    }
    return s.buffer
  })()
  return s.loading
}

function select(id) {
  editor.value?.stopPlayback()
  currentId.value = id
  const s = sources.value.find((x) => x.id === id)
  if (s) ensureDecoded(s)
}

function removeSource(id) {
  const i = sources.value.findIndex((s) => s.id === id)
  if (i < 0) return
  sources.value.splice(i, 1)
  if (currentId.value === id) {
    const next = sources.value[Math.min(i, sources.value.length - 1)]
    if (next) select(next.id)
    else currentId.value = null
  }
}

function pick() {
  fileInput.value?.click()
}
function onPick(e) {
  addFiles(e.target.files || [])
  e.target.value = ''
}

// ---------- 參數 ----------
const sel = ref({ start: 0, end: 0, duration: 0, total: 0 })
const speed = ref(1.5)
const trimmed = computed(() => sel.value.total > 0 && sel.value.duration < sel.value.total - 0.01)
const outSeconds = computed(() => outputSeconds(sel.value.duration, speed.value))
const tooLong = computed(() => !isUnitSpeed(speed.value) && sel.value.duration > MAX_STRETCH_SECONDS)
const channelLabel = (n) => (n === 1 ? '單聲道' : n === 2 ? '立體聲' : `${n} 聲道`)

function setSpeed(v) {
  speed.value = Math.round(clampSpeed(v) * 100) / 100
}

// ---------- 產生 ----------
const results = ref([]) // { id, name, blob, url, seconds, size, origin, returned }
const busy = ref('')

async function render(source, range, spd, isTrimmed) {
  const buf = await ensureDecoded(source)
  if (!buf) throw new Error(`${source.file.name}：${source.error}`)
  let { channels, sampleRate } = extractChannels(buf, range.start, range.end)
  if (!channels[0]?.length) throw new Error('選取範圍內沒有音訊')
  // 速度 1 就不要碰它：時間伸縮再怎麼準也是有損的，原樣通過才是對的。
  if (!isUnitSpeed(spd)) {
    if (channels[0].length / sampleRate > MAX_STRETCH_SECONDS) {
      throw new Error(`變速一次最多 ${MAX_STRETCH_SECONDS / 60} 分鐘：${source.file.name} 請先選短一點的範圍`)
    }
    channels = await stretchChannels(channels, sampleRate, 1 / clampSpeed(spd))
  }
  const blob = encodeWav(channels, sampleRate)
  const taken = new Set(results.value.map((r) => r.name))
  results.value.push({
    id: crypto.randomUUID(),
    name: uniqueName(suggestName(source.file.name, { speed: spd, trimmed: isTrimmed }), taken),
    blob: markRaw(blob),
    url: URL.createObjectURL(blob),
    seconds: channels[0].length / sampleRate,
    size: blob.size,
    origin: source.origin,
    returned: '', // '' | 'ok' | 'failed'
  })
  returnResult(results.value.at(-1))
}

/** 從別的工具來的音檔：每做出一個結果就傳回來源。每個結果都是新檔，不會互相取代。 */
async function returnResult(r) {
  if (!r.origin) return
  const ok = await sendResult(r.origin, r.id, new File([r.blob], r.name, { type: 'audio/wav' }))
  r.returned = ok ? 'ok' : 'failed'
}

const returnNote = (r) =>
  !r.origin || !r.returned ? ''
    : r.returned === 'ok' ? ` · 已傳回${toolLabel(r.origin.app)}`
      : ` · ${toolLabel(r.origin.app)}已關閉，沒傳回`

async function renderCurrent() {
  if (busy.value || !current.value) return
  editor.value?.stopPlayback()
  busy.value = isUnitSpeed(speed.value) ? '輸出中…' : '變速中…'
  try {
    await render(current.value, sel.value, speed.value, trimmed.value)
    toast('完成，結果在下方')
  } catch (err) {
    toast(err.message)
  } finally {
    busy.value = ''
  }
}

async function renderAll() {
  if (busy.value) return
  editor.value?.stopPlayback()
  const list = [...sources.value]
  let failed = 0
  for (const [i, s] of list.entries()) {
    busy.value = `處理中 ${i + 1}/${list.length}…`
    try {
      const buf = await ensureDecoded(s)
      if (!buf) throw new Error(s.error)
      await render(s, { start: 0, end: buf.duration }, speed.value, false)
    } catch (err) {
      console.warn('render failed', s.file.name, err)
      failed++
    }
  }
  busy.value = ''
  toast(failed ? `完成 ${list.length - failed} 個，${failed} 個失敗` : `${list.length} 個音檔都轉好了`)
}

function removeResult(r) {
  URL.revokeObjectURL(r.url)
  results.value = results.value.filter((x) => x !== r)
}

const zipName = () => `audiospeed-${timeStamp()}.zip`
const zipList = () => results.value.map((r) => ({ name: r.name, blob: r.blob }))

async function downloadAll() {
  saveBlob(await makeZip(zipList()), zipName())
}

const sending = ref(false)
async function sendAll() {
  if (sending.value || !results.value.length) return
  sending.value = true
  const list = zipList()
  // 同步呼叫：ZIP 編輯器沒開的話要在這次點擊裡開新分頁。
  const res = await sendToEditor(async () => new File([await makeZip(list)], zipName(), { type: 'application/zip' }))
  sending.value = false
  if (res === 'sent') toast(`已把 ${list.length} 個音檔傳到 ZIP 編輯器`)
  else if (res === 'blocked') toast('瀏覽器擋下了新分頁，請先開啟 ZIP 編輯器再傳送')
  else toast('ZIP 編輯器沒有回應，請再試一次')
}

// ---------- 拖放 ----------
const dragging = ref(false)
let dragDepth = 0
const hasFiles = (e) => !!e.dataTransfer && [...e.dataTransfer.types].includes('Files')
function onDragEnter(e) {
  if (!hasFiles(e)) return
  e.preventDefault()
  dragDepth++
  dragging.value = true
}
function onDragOver(e) {
  if (!hasFiles(e)) return
  e.preventDefault()
  e.dataTransfer.dropEffect = 'copy'
}
function onDragLeave(e) {
  if (hasFiles(e) && --dragDepth <= 0) {
    dragDepth = 0
    dragging.value = false
  }
}
function onDrop(e) {
  if (!hasFiles(e)) return
  e.preventDefault()
  dragDepth = 0
  dragging.value = false
  addFiles(e.dataTransfer.files)
}

let stopListening = () => {}
onMounted(() => {
  window.addEventListener('dragenter', onDragEnter)
  window.addEventListener('dragover', onDragOver)
  window.addEventListener('dragleave', onDragLeave)
  window.addEventListener('drop', onDrop)
  stopListening = listen((files, from, origins) => addFiles(files, from || 'zipeditor', origins))
})
onBeforeUnmount(() => {
  window.removeEventListener('dragenter', onDragEnter)
  window.removeEventListener('dragover', onDragOver)
  window.removeEventListener('dragleave', onDragLeave)
  window.removeEventListener('drop', onDrop)
  stopListening()
})
</script>

<template>
  <div class="app" :class="{ dragging }">
    <header class="top">
      <div class="brand">
        <svg viewBox="0 0 32 32" width="30" height="30" aria-hidden="true"><rect width="32" height="32" rx="8" fill="#c2410c"/><path d="M7 16h2M11 11v10M15 8v16M19 12v8M23 14v4" stroke="#fff" stroke-width="2.2" stroke-linecap="round"/></svg>
        <div>
          <h1>音檔變速</h1>
          <p v-if="!sources.length">加快或放慢音檔、音高不變，也能剪出其中一段。檔案不會離開你的裝置。</p>
        </div>
      </div>
    </header>

    <template v-if="!sources.length">
      <section class="start panel">
        <Icon name="music" class="hero" />
        <strong>把音檔拖進來</strong>
        <span>MP3、M4A、WAV、FLAC、OGG……瀏覽器播得了的格式都可以，一次可以放很多個</span>
        <div class="start-btns">
          <button type="button" class="btn primary" @click="pick"><Icon name="upload" />選擇音檔</button>
        </div>
      </section>

      <section class="about panel">
        <h2>可以做什麼</h2>
        <ul>
          <li><strong>變速不變調</strong>：0.25×～4×。Podcast、課程錄音加快聽，語言教材放慢聽，聲音不會變成花栗鼠或大叔。</li>
          <li><strong>剪出一段</strong>：在波形上拖曳選出範圍，先播放確認再輸出。</li>
          <li><strong>批次處理</strong>：一次放好幾個檔，用同一個速度整批轉好。</li>
          <li><strong>串接 ZIP 編輯器</strong>：在 <a href="../zipeditor/">ZIP 編輯器</a>勾選音檔就能傳過來，轉好再按「傳到 ZIP 編輯器」放回去。</li>
          <li><strong>不上傳</strong>：解碼、變速、輸出 WAV 都在瀏覽器裡完成。變速核心是 Rubber Band 演算法的 Rust 移植，編成 WebAssembly 執行。</li>
        </ul>
      </section>
    </template>

    <template v-else>
      <section class="panel section">
        <h2>
          音檔 <span class="count">{{ sources.length }}</span>
          <span class="grow"></span>
          <button type="button" class="btn small" :disabled="!!busy" @click="pick"><Icon name="plus" />加入音檔</button>
        </h2>
        <ul class="sources">
          <li v-for="s in sources" :key="s.id" :class="{ active: s.id === currentId }">
            <button type="button" class="pick" :disabled="!!busy" @click="select(s.id)">
              <Icon name="music" />
              <span class="name">{{ s.file.name }}</span>
              <span class="meta">{{ s.buffer ? fmtSeconds(s.buffer.duration) : s.error ? '無法讀取' : '' }}</span>
            </button>
            <button type="button" class="btn small icon" title="移除" :disabled="!!busy" @click="removeSource(s.id)"><Icon name="x" /></button>
          </li>
        </ul>
      </section>

      <section v-if="current" class="panel section">
        <h2 class="title">{{ current.file.name }}</h2>
        <div v-if="current.loading" class="banner"><span class="spinner" aria-hidden="true"></span>解碼中…</div>
        <div v-else-if="current.error" class="banner error">{{ current.error }}</div>
        <template v-else-if="current.buffer">
          <WaveEditor ref="editor" :buffer="current.buffer" :audio-ctx="audioCtx" :disabled="!!busy" @update="(v) => (sel = v)" />

          <div class="speed">
            <span class="label">速度</span>
            <div class="presets">
              <button v-for="p in SPEED_PRESETS" :key="p" type="button" class="btn small"
                      :class="{ on: Math.abs(speed - p) < 1e-6 }" @click="setSpeed(p)">{{ p }}×</button>
            </div>
            <div class="fine">
              <input type="range" :min="MIN_SPEED" :max="MAX_SPEED" step="0.05" :value="speed" aria-label="速度"
                     @input="setSpeed($event.target.value)" />
              <input type="number" :min="MIN_SPEED" :max="MAX_SPEED" step="0.05" :value="speed" aria-label="速度（倍）"
                     @change="setSpeed($event.target.value)" />
              <span>×</span>
            </div>
          </div>

          <div class="render">
            <span class="grow out">
              輸出 <strong>{{ fmtSeconds(outSeconds) }}</strong><span class="muted">&nbsp;· {{ current.buffer.sampleRate / 1000 }} kHz · {{ channelLabel(current.buffer.numberOfChannels) }} · WAV</span>
            </span>
            <button v-if="sources.length > 1" type="button" class="btn" :disabled="!!busy" @click="renderAll">
              全部以 {{ speed }}× 整段轉換
            </button>
            <button type="button" class="btn primary" :disabled="!!busy || tooLong || sel.duration <= 0" @click="renderCurrent">
              <span v-if="busy" class="spinner" aria-hidden="true"></span><Icon v-else name="gauge" />{{ busy || '產生音檔' }}
            </button>
          </div>
          <p v-if="tooLong" class="warn">變速一次最多 {{ MAX_STRETCH_SECONDS / 60 }} 分鐘，請在波形上選短一點的範圍。</p>
        </template>
      </section>

      <section v-if="results.length" class="panel section">
        <h2>結果 <span class="count">{{ results.length }}</span></h2>
        <ul class="results">
          <li v-for="r in results" :key="r.id">
            <div class="info">
              <b>{{ r.name }}</b>
              <span>{{ fmtSeconds(r.seconds) }} · {{ fmtBytes(r.size) }}{{ returnNote(r) }}</span>
            </div>
            <audio :src="r.url" controls preload="metadata"></audio>
            <div class="row-btns">
              <button type="button" class="btn small icon" title="下載" @click="saveBlob(r.blob, r.name)"><Icon name="download" /></button>
              <button type="button" class="btn small icon danger" title="移除" @click="removeResult(r)"><Icon name="trash" /></button>
            </div>
          </li>
        </ul>
        <div class="actions">
          <button v-if="results.length > 1" type="button" class="btn" @click="downloadAll"><Icon name="download" />打包下載</button>
          <button v-if="bridgeSupported" type="button" class="btn primary" :disabled="sending" @click="sendAll">
            <Icon name="send" />傳到 ZIP 編輯器
          </button>
        </div>
      </section>
    </template>

    <footer>
      音檔變速 · 所有運算都在瀏覽器內完成 ·
      <a href="../zipeditor/">ZIP 編輯器</a> ·
      <a href="../">更多工具</a> ·
      <a href="https://github.com/mason276752/audiospeed">GitHub 原始碼</a>
    </footer>

    <div class="toast" :class="{ show: toastMsg }" role="status" aria-live="polite">
      <span>{{ toastMsg }}</span>
    </div>

    <input ref="fileInput" type="file" accept="audio/*,video/*" multiple hidden @change="onPick" />
  </div>
</template>

<style scoped>
.sources { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; }
.sources li { display: flex; align-items: center; gap: 6px; }
.pick {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; padding: 7px 10px;
  border: 1px solid var(--line); border-radius: 10px; background: none; color: var(--text);
  font: inherit; font-size: 14px; text-align: left; cursor: pointer;
}
.pick:hover { background: var(--hover); }
.pick:disabled { cursor: default; }
.active .pick { border-color: var(--accent); background: var(--accent-soft); }
.pick .icon { color: var(--accent); }
.name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.meta { color: var(--muted); font-size: 13px; font-variant-numeric: tabular-nums; }

.title { overflow-wrap: anywhere; }
.speed { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 14px; margin-top: 16px; }
.speed .label { font-weight: 600; }
.presets { display: flex; flex-wrap: wrap; gap: 6px; }
.fine { display: flex; align-items: center; gap: 8px; }
.fine input[type="range"] { width: 160px; }
.fine input[type="number"] { width: 76px; }
.render { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 10px; margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--line); }
.out strong { font-variant-numeric: tabular-nums; }
.muted { color: var(--muted); }
.warn { margin: 8px 0 0; color: var(--danger); font-size: 13px; }

.results { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
.results li {
  display: grid; grid-template-columns: minmax(0, 1fr) minmax(200px, 320px) auto; align-items: center; gap: 8px 12px;
  padding: 8px 10px; border: 1px solid var(--line); border-radius: 10px;
}
.info { display: grid; min-width: 0; }
.info b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
.info span { color: var(--muted); font-size: 13px; }
.results audio { width: 100%; height: 36px; }
.row-btns { display: flex; gap: 6px; }
.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-top: 12px; }
@media (max-width: 640px) {
  .results li { grid-template-columns: minmax(0, 1fr) auto; }
  .results audio { grid-column: 1 / -1; grid-row: 2; }
}
</style>
