// 波形上的「選取片段」數學（純函式，沒有 canvas、沒有 Vue）。
//
// 四種拖曳（拉左把手、拉右把手、從空白拉出新選取、整段平移）各有各的夾擠規則，而錯了
// 的症狀都是「選取自己亂跳」——很難靠肉眼在畫布上除錯，所以整組抽出來測。
//
// 座標一律是**秒**；把手的命中範圍是像素，所以 `zoneOf` 才需要 pxPerSec。

/** 把手的命中半徑（螢幕像素）。 */
export const HANDLE_HIT_PX = 9
/** 選取的最短長度（秒）——拉到 0 會讓匯出變成空檔。 */
export const MIN_SELECTION = 0.02

/**
 * 指標落在哪個可互動區域。
 *
 * 把手優先於「選取區內」：兩者在邊緣一定重疊，先判把手才拖得動邊界（否則永遠只會整段平移）。
 * @returns {'start'|'end'|'move'|null}
 */
export function zoneOf(sec, sel, pxPerSec) {
  if (Math.abs(sec - sel.start) * pxPerSec < HANDLE_HIT_PX) return 'start'
  if (Math.abs(sec - sel.end) * pxPerSec < HANDLE_HIT_PX) return 'end'
  if (sec > sel.start && sec < sel.end) return 'move'
  return null
}

/**
 * 拖曳到 `sec` 之後的新選取範圍。
 *
 * @param {string} mode 'start' | 'end' | 'new' | 'move'
 * @param {{start:number,end:number}} sel 目前選取
 * @param {number} sec 指標所在秒數
 * @param {number} duration 全曲長度
 * @param {object} drag `{ anchor }`（new 模式的固定端）／`{ offset }`（move 模式的抓取位移）
 */
export function applyDrag(mode, sel, sec, duration, drag = {}) {
  let { start, end } = sel

  if (mode === 'start') {
    start = Math.min(sec, end - MIN_SELECTION)
  } else if (mode === 'end') {
    end = Math.max(sec, start + MIN_SELECTION)
  } else if (mode === 'new') {
    // 從空白處拉出新選取：起點固定在 anchor，游標往左／往右都能拉。
    start = Math.min(drag.anchor, sec)
    end = Math.max(drag.anchor, sec)
  } else if (mode === 'move') {
    // 整段平移：長度不變，並且不許滑出兩端（不然會愈拖愈短）。
    const len = end - start
    start = Math.max(0, Math.min(duration - len, sec - drag.offset))
    end = start + len
  }

  return { start: Math.max(0, start), end: Math.min(duration, end) }
}

/** 指標的 x（畫布內像素）→ 秒；超出畫布會被夾在兩端。 */
export function xToSeconds(x, width, duration) {
  const clamped = Math.max(0, Math.min(width, x))
  return width > 0 ? (clamped / width) * duration : 0
}
