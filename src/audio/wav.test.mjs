/**
 * wav 的單元測試。
 *
 * 這是**使用者最後拿到的那份檔案**——選段裁切、聲道交錯、16-bit 量化都在這裡。
 * 寫錯的症狀是一段雜訊或長度不對的音檔，而畫面上一切正常。
 *
 * WAV 標頭的每個欄位都逐一驗：位元組錯一個，任何播放器就整份讀不出來。
 */
import { test } from 'vitest'
import assert from 'node:assert/strict'
import { encodeWav, extractChannels, extractMono, wholeMono } from './wav.js'

/** 把 Blob 讀成 DataView。 */
async function view(blob) {
  return new DataView(await blob.arrayBuffer())
}
const ascii = (dv, off, len) =>
  Array.from({ length: len }, (_, i) => String.fromCharCode(dv.getUint8(off + i))).join('')

/** 造一個 n 聲道的假 AudioBuffer。 */
function buffer(channels, sampleRate = 8000) {
  const length = channels[0].length
  return {
    sampleRate,
    length,
    duration: length / sampleRate,
    numberOfChannels: channels.length,
    getChannelData: (c) => channels[c],
  }
}

// ---- WAV 標頭 ----

test('RIFF/WAVE 標頭的每個欄位都對（單聲道）', async () => {
  const dv = await view(encodeWav([new Float32Array(4)], 16000))
  assert.equal(ascii(dv, 0, 4), 'RIFF')
  assert.equal(dv.getUint32(4, true), 36 + 4 * 2, '檔案長度 − 8')
  assert.equal(ascii(dv, 8, 4), 'WAVE')
  assert.equal(ascii(dv, 12, 4), 'fmt ')
  assert.equal(dv.getUint32(16, true), 16, 'fmt chunk 大小')
  assert.equal(dv.getUint16(20, true), 1, 'PCM')
  assert.equal(dv.getUint16(22, true), 1, '單聲道')
  assert.equal(dv.getUint32(24, true), 16000, '取樣率')
  assert.equal(dv.getUint32(28, true), 32000, 'byte rate = 取樣率 × 2')
  assert.equal(dv.getUint16(32, true), 2, 'block align')
  assert.equal(dv.getUint16(34, true), 16, 'bits per sample')
  assert.equal(ascii(dv, 36, 4), 'data')
  assert.equal(dv.getUint32(40, true), 8, 'data chunk 大小 = 樣本數 × 2')
})

test('立體聲的 byte rate、block align 與 data 大小都乘上聲道數', async () => {
  const dv = await view(encodeWav([new Float32Array(3), new Float32Array(3)], 44100))
  assert.equal(dv.getUint16(22, true), 2)
  assert.equal(dv.getUint32(28, true), 44100 * 4)
  assert.equal(dv.getUint16(32, true), 4)
  assert.equal(dv.getUint32(40, true), 12)
  assert.equal(dv.getUint32(4, true), 36 + 12)
})

test('總長度 = 44 位元組標頭 + 每個樣本 2 位元組', async () => {
  const blob = encodeWav([new Float32Array(100)], 8000)
  assert.equal(blob.size, 44 + 200)
  assert.equal(blob.type, 'audio/wav')
})

test('空樣本也產得出一份合法（只有標頭）的 WAV', async () => {
  const blob = encodeWav([new Float32Array(0)], 8000)
  assert.equal(blob.size, 44)
  const dv = await view(blob)
  assert.equal(dv.getUint32(40, true), 0)
})

// ---- 量化與交錯 ----

test('滿刻度的正負值分別對到 +32767 / −32768', async () => {
  const dv = await view(encodeWav([new Float32Array([1, -1])], 8000))
  assert.equal(dv.getInt16(44, true), 32767)
  assert.equal(dv.getInt16(46, true), -32768)
})

test('超出 [-1,1] 的值會被夾住，不會 wrap 成反相的爆音', async () => {
  const dv = await view(encodeWav([new Float32Array([5, -5])], 8000))
  assert.equal(dv.getInt16(44, true), 32767, '超過 1 應削峰，不是繞回負值')
  assert.equal(dv.getInt16(46, true), -32768)
})

test('多聲道按「樣本 0 的各聲道、樣本 1 的各聲道……」交錯寫入', async () => {
  const left = new Float32Array([1, 0])
  const right = new Float32Array([-1, 0.5])
  const dv = await view(encodeWav([left, right], 8000))
  assert.equal(dv.getInt16(44, true), 32767, '樣本 0 左')
  assert.equal(dv.getInt16(46, true), -32768, '樣本 0 右')
  assert.equal(dv.getInt16(48, true), 0, '樣本 1 左')
  assert.equal(dv.getInt16(50, true), Math.trunc(0.5 * 0x7fff), '樣本 1 右')
})

// ---- 選段裁切 ----

test('extractChannels 保留每個聲道、只取指定區間（左閉右開）', () => {
  const l = Float32Array.from({ length: 10 }, (_, i) => i / 10)
  const r = Float32Array.from({ length: 10 }, (_, i) => -i / 10)
  const { channels, sampleRate } = extractChannels(buffer([l, r], 10), 0.2, 0.5)
  assert.equal(sampleRate, 10)
  assert.equal(channels.length, 2)
  assert.equal(channels[0].length, 3, '0.2~0.5 秒 @10Hz = 3 個樣本')
  assert.ok(Math.abs(channels[0][0] - 0.2) < 1e-6)
  assert.ok(Math.abs(channels[1][0] + 0.2) < 1e-6)
})

test('extractChannels 回的是複本，改它不會動到原本的音訊', () => {
  const ch = new Float32Array([0.1, 0.2, 0.3])
  const { channels } = extractChannels(buffer([ch], 1), 0, 3)
  channels[0][0] = 9
  assert.ok(Math.abs(ch[0] - 0.1) < 1e-6)
})

test('extractMono 把多聲道平均成單聲道', () => {
  const { samples } = extractMono(buffer([new Float32Array([1, 1]), new Float32Array([0, 0])], 2), 0, 1)
  assert.ok(Math.abs(samples[0] - 0.5) < 1e-6, '兩聲道平均應為 0.5')
})

test('夾在音檔範圍內（拖過頭不會讀到界外）', () => {
  const ch = new Float32Array([0.1, 0.2, 0.3])
  assert.equal(extractMono(buffer([ch], 1), -5, 999).samples.length, 3)
  assert.equal(extractChannels(buffer([ch], 1), -5, 999).channels[0].length, 3)
})

test('起點在終點之後時回空陣列，不會回負長度', () => {
  const ch = new Float32Array(10)
  assert.equal(extractMono(buffer([ch], 10), 0.8, 0.2).samples.length, 0)
  assert.equal(extractChannels(buffer([ch], 10), 0.8, 0.2).channels[0].length, 0)
})

test('wholeMono 取整段', () => {
  const ch = new Float32Array([0.1, 0.2, 0.3, 0.4])
  assert.equal(wholeMono(buffer([ch], 4)).length, 4)
})

// ---- 端到端 ----

test('選段 → 編碼：長度與取樣率一路對得起來', async () => {
  const ch = Float32Array.from({ length: 96000 }, () => 0.25) // 2 秒 @48kHz
  const { channels, sampleRate } = extractChannels(buffer([ch, ch], 48000), 1, 1.5) // 取中間 0.5 秒
  assert.equal(channels[0].length, 24000)

  const dv = await view(encodeWav(channels, sampleRate))
  assert.equal(dv.getUint32(24, true), 48000, '取樣率要跟著原檔走')
  assert.equal(dv.getUint32(40, true), 24000 * 2 * 2, 'data 大小 = 24000 樣本 × 2 聲道 × 2 位元組')
})
