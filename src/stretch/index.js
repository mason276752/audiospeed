/**
 * 時間伸縮（音高不變）。每次開一個新的 Worker、算完就關掉：WASM 的記憶體只會長不會縮，
 * 一段長音檔用掉的幾百 MB 不該在之後一直佔著。
 *
 * @param {Float32Array[]} channels 每聲道一條；**會被轉移給 Worker**，呼叫後不能再用
 * @param {number} rate 取樣率
 * @param {number} ratio 輸出／輸入時長比（速度的倒數）
 * @returns {Promise<Float32Array[]>}
 */
export function stretchChannels(channels, rate, ratio) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' })
    worker.onmessage = (e) => {
      worker.terminate()
      const { channels: out, error, oom } = e.data
      if (out) resolve(out)
      else reject(new Error(oom ? '記憶體不夠，請選短一點的範圍再試' : `變速失敗：${error}`))
    }
    worker.onerror = (e) => {
      worker.terminate()
      reject(new Error(`變速失敗：${e.message || '無法啟動運算'}`))
    }
    worker.postMessage({ channels, rate, ratio }, channels.map((c) => c.buffer))
  })
}
