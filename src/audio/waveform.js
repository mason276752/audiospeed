// 波形的取樣與繪製。
//
// 取樣（`computePeaks`）與繪製分開：每個畫布像素要畫的是「這一段取樣裡的最小/最大值」
// 這條垂直線，而那段 min/max 的算法是純數學、也是唯一會算錯的地方（算錯就是波形被抹平
// 或漏掉尖峰），所以它獨立出來測。繪製本身只是把算好的數字塗上去。

/** 波形高度（CSS 像素）。 */
export const WAVE_HEIGHT = 128
/** 首次載入時把手發光提示的長度（毫秒）。 */
export const PULSE_MS = 2600

/**
 * 把整條單聲道取樣壓縮成每個像素一組 `[min, max]`。
 *
 * @param {Float32Array|number[]} mono 全曲單聲道取樣（值域 -1~1）
 * @param {number} width 畫布寬度（像素）
 * @returns {{min: Float32Array, max: Float32Array}}
 */
export function computePeaks(mono, width) {
  const min = new Float32Array(width)
  const max = new Float32Array(width)
  if (!mono?.length || width <= 0) return { min, max }

  const step = mono.length / width
  for (let x = 0; x < width; x++) {
    const i0 = Math.floor(x * step)
    const i1 = Math.min(mono.length, Math.floor((x + 1) * step))
    // 取樣數少於像素數時，某些像素分不到任何取樣——畫成 0（靜音）而不是留下上一格的殘值。
    if (i1 <= i0) continue
    let mn = 1
    let mx = -1
    for (let i = i0; i < i1; i++) {
      const v = mono[i]
      if (v < mn) mn = v
      if (v > mx) mx = v
    }
    min[x] = mn
    max[x] = mx
  }
  return { min, max }
}

/**
 * 波形的配色。從 CSS 變數讀，亮色／暗色主題才會跟著頁面走
 * （原本寫死的是暗色那一套，亮色背景上會看不清楚）。
 */
export function waveColors(el) {
  const css = getComputedStyle(el)
  const v = (name, fallback) => css.getPropertyValue(name).trim() || fallback
  return {
    selected: v('--accent', '#d9480f'),
    idle: v('--wave-idle', '#9aa3b2'),
    shade: v('--wave-shade', 'rgba(15,23,42,.45)'),
    handle: v('--accent', '#d9480f'),
    handleActive: v('--accent-strong', '#f97316'),
    playhead: v('--danger', '#dc2626'),
  }
}

/**
 * 畫一個「可拖曳把手」：膠囊形＋握把紋（三個小點），比單純一條色線更明顯是可拖曳的 UI。
 *
 * `x` 應已 clamp 過（見 `drawWaveform`），故選取在畫布最左/右緣時把手仍完整可見。
 * `glow`：首次載入的短暫提示動畫強度（0~1）。
 */
export function drawHandle(g, x, mid, active, glow, colors) {
  const hh = 20 // 把手半高
  if (glow > 0) {
    g.save()
    g.globalAlpha = glow * 0.5
    g.fillStyle = colors.handleActive
    g.beginPath()
    if (g.roundRect) g.roundRect(x - 9, mid - hh - 5, 18, hh * 2 + 10, 10)
    else g.rect(x - 9, mid - hh - 5, 18, hh * 2 + 10)
    g.fill()
    g.restore()
  }
  const w = active ? 13 : 11
  g.save()
  g.shadowColor = 'rgba(0,0,0,.4)'
  g.shadowBlur = active ? 5 : 2
  g.fillStyle = active ? colors.handleActive : colors.handle
  g.beginPath()
  if (g.roundRect) g.roundRect(x - w / 2, mid - hh, w, hh * 2, w / 2)
  else g.rect(x - w / 2, mid - hh, w, hh * 2)
  g.fill()
  g.restore()
  g.fillStyle = 'rgba(255,255,255,.75)'
  for (const dy of [-6, 0, 6]) {
    g.beginPath()
    g.arc(x, mid + dy, 1.2, 0, Math.PI * 2)
    g.fill()
  }
}

/** 首次載入的短暫發光提示強度（讓把手一開始就顯眼，不用等使用者發現要 hover）。 */
export function pulseGlow(pulseStart, now) {
  if (!pulseStart) return 0
  const t = now - pulseStart
  if (t >= PULSE_MS) return 0
  return Math.max(0, 1 - t / PULSE_MS) * (0.55 + 0.45 * Math.sin(t / 130))
}

/**
 * 畫整張波形：取樣線、未選取區的淡化、邊界線、兩個把手、播放頭。
 *
 * @param {CanvasRenderingContext2D} g
 * @param {object} view
 */
export function drawWaveform(g, view) {
  const { mono, width: w, duration, sel, activeStart, activeEnd, glow, playhead, colors } = view
  const h = WAVE_HEIGHT
  const mid = h / 2
  g.clearRect(0, 0, w, h)
  if (!mono || !duration) return

  const sec2x = (s) => (s / duration) * w
  const selX0 = sec2x(sel.start)
  const selX1 = sec2x(sel.end)

  const { min, max } = computePeaks(mono, w)
  for (let x = 0; x < w; x++) {
    g.strokeStyle = x >= selX0 && x <= selX1 ? colors.selected : colors.idle
    g.beginPath()
    g.moveTo(x + 0.5, mid - max[x] * mid * 0.94)
    g.lineTo(x + 0.5, mid - min[x] * mid * 0.94)
    g.stroke()
  }

  // 未選取區域淡化
  g.fillStyle = colors.shade
  if (selX0 > 0) g.fillRect(0, 0, selX0, h)
  if (selX1 < w) g.fillRect(selX1, 0, w - selX1, h)
  // 邊界線
  g.fillStyle = colors.selected
  g.fillRect(selX0 - 1, 0, 2, h)
  g.fillRect(selX1 - 1, 0, 2, h)

  // 把手（clamp 到畫布內，選取貼邊時仍完整可見）
  drawHandle(g, Math.max(9, Math.min(w - 9, selX0)), mid, activeStart, glow, colors)
  drawHandle(g, Math.max(9, Math.min(w - 9, selX1)), mid, activeEnd, glow, colors)

  if (playhead != null) {
    g.fillStyle = colors.playhead
    g.fillRect(sec2x(playhead) - 1, 0, 2, h)
  }
}
