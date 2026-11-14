/**
 * waveform 的取樣測試（`npm test`）。
 *
 * `computePeaks` 是「一個像素一條垂直線」的來源。算錯不會報錯，只會讓波形看起來被抹平
 * 或漏掉尖峰——而那正是使用者判斷「要裁哪一段」的唯一依據。
 */
import { test } from 'vitest'
import assert from 'node:assert/strict'
import { PULSE_MS, computePeaks, pulseGlow } from './waveform.js'

test('每個像素取到那一段的最小與最大值', () => {
  const mono = new Float32Array([0, 1, -1, 0.5])
  const { min, max } = computePeaks(mono, 2)
  assert.deepEqual([...max], [1, 0.5])
  assert.deepEqual([...min], [0, -1])
})

test('尖峰不會被平均掉（波形靠的就是這個）', () => {
  const mono = new Float32Array(1000).fill(0)
  mono[500] = 0.9 // 只有一個取樣是尖峰
  const { max } = computePeaks(mono, 10)
  // Float32Array 存不下精確的 0.9，所以比近似值
  assert.ok(Math.abs(Math.max(...max) - 0.9) < 1e-6, `尖峰被抹平了：${Math.max(...max)}`)
})

test('輸出長度等於畫布寬度', () => {
  const { min, max } = computePeaks(new Float32Array(1000), 137)
  assert.equal(min.length, 137)
  assert.equal(max.length, 137)
})

test('取樣數少於像素數時，分不到取樣的像素畫成靜音（不是殘留上一格）', () => {
  const mono = new Float32Array([1, -1])
  const { min, max } = computePeaks(mono, 8)
  // 每個像素要嘛拿到取樣、要嘛是 0，不會出現「整排都是 1」
  assert.ok(max.some((v) => v === 0), '應有分不到取樣的像素被畫成 0')
  assert.equal(max.length, 8)
  assert.equal(min.length, 8)
})

test('空音訊／零寬度都回等長的零陣列，不丟例外', () => {
  assert.deepEqual([...computePeaks(new Float32Array(0), 4).max], [0, 0, 0, 0])
  assert.deepEqual([...computePeaks(null, 3).min], [0, 0, 0])
  assert.equal(computePeaks(new Float32Array([1]), 0).max.length, 0)
})

test('全靜音的波形每格都是 0', () => {
  const { min, max } = computePeaks(new Float32Array(100), 10)
  assert.ok([...min, ...max].every((v) => v === 0))
})

test('pulseGlow：沒有動畫時回 0', () => {
  assert.equal(pulseGlow(0, 1000), 0)
})

test('pulseGlow：動畫期間為正值、結束後歸零', () => {
  assert.ok(pulseGlow(1000, 1000) > 0)
  assert.equal(pulseGlow(1000, 1000 + PULSE_MS), 0)
  assert.equal(pulseGlow(1000, 1000 + PULSE_MS + 500), 0)
})

test('pulseGlow 整體隨時間衰減（提示會自己淡掉，不會一直閃）', () => {
  // pulseStart 為 0 代表「沒有動畫」，所以起點要用非 0 的時間戳
  const early = pulseGlow(1, 1 + 100)
  const late = pulseGlow(1, 1 + PULSE_MS - 100)
  assert.ok(late < early, `應該衰減：${early} → ${late}`)
})
