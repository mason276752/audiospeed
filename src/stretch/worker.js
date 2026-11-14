// 在 Web Worker 裡跑時間伸縮（stretch.wasm，介面見 stretch/src/lib.rs）。
// 整段算完才回，十分鐘的音檔要好幾秒，放在主執行緒會讓頁面整個卡住。
import wasmUrl from './stretch.wasm?url'

self.onmessage = async (e) => {
  const { channels, rate, ratio } = e.data
  try {
    const { instance } = await WebAssembly.instantiateStreaming(fetch(wasmUrl))
    const w = instance.exports
    const ch = channels.length
    const len = channels[0].length
    const inPtr = w.alloc(ch * len)
    // 每次都從 memory.buffer 重新取：配置可能讓記憶體成長，先前的 ArrayBuffer 就失效了。
    const input = new Float32Array(w.memory.buffer, inPtr, ch * len)
    channels.forEach((c, i) => input.set(c, i * len))
    const result = w.stretch_planar(inPtr, ch, len, rate, ratio)
    w.dealloc(inPtr, ch * len)
    const n = w.stretched_len(len, ratio)
    const out = []
    for (let c = 0; c < ch; c++) out.push(new Float32Array(w.memory.buffer, w.result_channel(result, c), n).slice())
    w.free_result(result)
    self.postMessage({ channels: out }, out.map((o) => o.buffer))
  } catch (err) {
    // 配置不到記憶體時 Rust 會 abort，在 JS 這邊是一個 `RuntimeError: unreachable`。
    self.postMessage({ error: String(err?.message || err), oom: err instanceof WebAssembly.RuntimeError })
  }
}
