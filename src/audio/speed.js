// 變速的純計算：速度範圍、輸出長度、檔名建議。
//
// 抽出來是因為這幾件事**錯了不會報錯，只會存出奇怪的東西**：速度沒夾在範圍內、
// 輸出檔名跟來源一樣（打包進 ZIP 時蓋掉原檔）。純函式才測得到這些邊界。

/** 速度快捷鍵。1 之外的值都會走時間伸縮（音高不變）。 */
export const SPEED_PRESETS = [0.5, 0.75, 1, 1.25, 1.5, 2]
/** 超過 4 倍音檔已經聽不出內容，低於 0.25 倍相位聲碼器的瑕疵會很明顯。 */
export const MIN_SPEED = 0.25
export const MAX_SPEED = 4
/**
 * 變速一次最多處理幾秒。時間伸縮整段在 WASM 記憶體裡做，放慢時輸出是輸入的好幾倍——
 * 10 分鐘立體聲放慢到 0.5 倍就要 1GB 上下，再長很容易配置失敗（手機更早）。
 */
export const MAX_STRETCH_SECONDS = 600

/** 夾到可用範圍；非數字回 1（＝不變速）。 */
export function clampSpeed(v) {
  const n = Number(v)
  if (!Number.isFinite(n)) return 1
  return Math.min(MAX_SPEED, Math.max(MIN_SPEED, n))
}

/** 這個速度算不算「不變速」（浮點數比較，別直接 === 1）。 */
export function isUnitSpeed(speed) {
  return Math.abs(clampSpeed(speed) - 1) < 1e-6
}

/** 選取 `seconds` 秒、用 `speed` 倍速播放後的長度。 */
export function outputSeconds(seconds, speed) {
  const s = clampSpeed(speed)
  return Math.max(0, Number(seconds) || 0) / s
}

/** 秒數 → 給人看的字串（跟波形上的格式一致）。 */
export function fmtSeconds(s) {
  const v = Math.max(0, Number(s) || 0)
  if (v < 60) return v.toFixed(1) + ' 秒'
  return `${Math.floor(v / 60)}:${String(Math.floor(v % 60)).padStart(2, '0')}`
}

/**
 * 依來源檔名與這次的編輯，建議一個新檔名。
 *
 * **不會建議原檔名**：這裡產生的一定是新的東西（剪過或變速過），而傳回 ZIP 編輯器合併時
 * 同名檔會蓋掉原始素材。
 */
export function suggestName(sourceName, { speed = 1, trimmed = false } = {}) {
  const base = String(sourceName || '音檔')
    .split(/[\\/]/)
    .pop()
    .replace(/\.[^.]+$/, '') || '音檔'
  const marks = []
  if (trimmed) marks.push('剪輯')
  if (!isUnitSpeed(speed)) marks.push(`${Number(clampSpeed(speed).toFixed(2))}x`)
  if (!marks.length) marks.push('副本')
  return `${base}-${marks.join('-')}.wav`
}
