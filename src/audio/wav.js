// WAV 編碼與選段裁切。
//
// 原本只有單聲道（送去語音生成用）；這裡輸出的是使用者要留著的音檔，所以保留原本的
// 聲道數，樣本交錯（interleaved）寫成 16-bit PCM。

/**
 * 把各聲道的 Float32 樣本編成 16-bit PCM WAV。
 *
 * @param {Float32Array[]} channels 每聲道一條（值域 [-1,1]），長度須一致
 * @param {number} sampleRate 取樣率（Hz）
 */
export function encodeWav(channels, sampleRate) {
  const ch = channels.length
  const n = ch ? channels[0].length : 0
  const dataSize = n * ch * 2
  const buf = new ArrayBuffer(44 + dataSize)
  const dv = new DataView(buf)
  const wr = (off, s) => { for (let i = 0; i < s.length; i++) dv.setUint8(off + i, s.charCodeAt(i)) }

  wr(0, 'RIFF')
  dv.setUint32(4, 36 + dataSize, true) // 檔案長度 - 8
  wr(8, 'WAVE')
  wr(12, 'fmt ')
  dv.setUint32(16, 16, true)        // fmt chunk 大小
  dv.setUint16(20, 1, true)         // PCM
  dv.setUint16(22, ch, true)        // 聲道數
  dv.setUint32(24, sampleRate, true)
  dv.setUint32(28, sampleRate * ch * 2, true) // byte rate
  dv.setUint16(32, ch * 2, true)    // block align
  dv.setUint16(34, 16, true)        // bits per sample
  wr(36, 'data')
  dv.setUint32(40, dataSize, true)

  let o = 44
  for (let i = 0; i < n; i++) {
    for (let c = 0; c < ch; c++) {
      const s = Math.max(-1, Math.min(1, channels[c][i]))
      dv.setInt16(o, s < 0 ? s * 0x8000 : s * 0x7fff, true)
      o += 2
    }
  }
  return new Blob([buf], { type: 'audio/wav' })
}

/** 秒 → 樣本區間 [s, e)，夾在音檔範圍內；起點在終點之後時長度為 0。 */
function sampleRange(buffer, startSec, endSec) {
  const sr = buffer.sampleRate
  const s = Math.max(0, Math.floor(startSec * sr))
  const e = Math.min(buffer.length, Math.floor(endSec * sr))
  return [s, Math.max(s, e)]
}

/** 從 AudioBuffer 取出 [startSec, endSec) 的片段，每聲道一條。 */
export function extractChannels(buffer, startSec, endSec) {
  const [s, e] = sampleRange(buffer, startSec, endSec)
  const channels = []
  for (let c = 0; c < buffer.numberOfChannels; c++) channels.push(buffer.getChannelData(c).slice(s, e))
  return { channels, sampleRate: buffer.sampleRate }
}

/** 從 AudioBuffer 取出 [startSec, endSec) 的片段，混音成單聲道 Float32。 */
export function extractMono(buffer, startSec, endSec) {
  const [s, e] = sampleRange(buffer, startSec, endSec)
  const len = e - s
  const out = new Float32Array(len)
  const ch = buffer.numberOfChannels
  for (let c = 0; c < ch; c++) {
    const d = buffer.getChannelData(c)
    for (let i = 0; i < len; i++) out[i] += d[s + i] / ch
  }
  return { samples: out, sampleRate: buffer.sampleRate }
}

/** 整段 AudioBuffer 混音成單聲道（畫波形用）。 */
export function wholeMono(buffer) {
  return extractMono(buffer, 0, buffer.duration).samples
}
