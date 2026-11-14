/**
 * selection 的單元測試（`npm test`）。
 *
 * 四種拖曳的夾擠規則。錯了的症狀是「選取自己亂跳」或「拖到後來變成空的」，在畫布上很
 * 難用肉眼除錯——所以每一條邊界都在這裡釘死。
 */
import { test } from 'vitest'
import assert from 'node:assert/strict'
import { HANDLE_HIT_PX, MIN_SELECTION, applyDrag, xToSeconds, zoneOf } from './selection.js'

const SEL = { start: 2, end: 6 }
const PX_PER_SEC = 10 // 1 秒 = 10px，故命中半徑 9px ≈ 0.9 秒

test('zoneOf 認得左右把手', () => {
  assert.equal(zoneOf(2, SEL, PX_PER_SEC), 'start')
  assert.equal(zoneOf(6, SEL, PX_PER_SEC), 'end')
})

test('zoneOf 的命中範圍是像素、不是秒（縮放後仍好按）', () => {
  // 0.5 秒外 = 5px，還在 9px 命中範圍內
  assert.equal(zoneOf(2.5, SEL, PX_PER_SEC), 'start')
  // 同樣是 0.5 秒，但每秒 100px → 50px，超出範圍，落回「選取區內」
  assert.equal(zoneOf(2.5, SEL, 100), 'move')
})

test('zoneOf：選取區內回 move', () => {
  assert.equal(zoneOf(4, SEL, PX_PER_SEC), 'move')
})

test('zoneOf：選取區外回 null（＝從空白拉出新選取）', () => {
  assert.equal(zoneOf(0.5, SEL, PX_PER_SEC), null)
  assert.equal(zoneOf(9, SEL, PX_PER_SEC), null)
})

test('zoneOf：把手優先於選取區內——否則永遠拖不動邊界', () => {
  // 這個位置同時在選取區內、也在 start 把手的命中範圍內
  const sec = 2 + (HANDLE_HIT_PX - 1) / PX_PER_SEC / 2
  assert.equal(zoneOf(sec, SEL, PX_PER_SEC), 'start')
})

test('拖左把手：往右拖不會越過右端，會留下最小長度', () => {
  const r = applyDrag('start', SEL, 99, 10)
  assert.equal(r.start, 6 - MIN_SELECTION)
  assert.equal(r.end, 6)
})

test('拖左把手：往左拖不會跑到 0 以前', () => {
  assert.deepEqual(applyDrag('start', SEL, -5, 10), { start: 0, end: 6 })
})

test('拖右把手：往左拖不會越過左端', () => {
  const r = applyDrag('end', SEL, 0, 10)
  assert.equal(r.end, 2 + MIN_SELECTION)
})

test('拖右把手：往右拖不會超過全曲長度', () => {
  assert.deepEqual(applyDrag('end', SEL, 999, 10), { start: 2, end: 10 })
})

test('從空白拉出新選取：往右拉', () => {
  assert.deepEqual(applyDrag('new', { start: 3, end: 3 }, 8, 10, { anchor: 3 }), { start: 3, end: 8 })
})

test('從空白拉出新選取：往左拉（起點固定在 anchor）', () => {
  assert.deepEqual(applyDrag('new', { start: 7, end: 7 }, 1, 10, { anchor: 7 }), { start: 1, end: 7 })
})

test('整段平移：長度不變', () => {
  const r = applyDrag('move', SEL, 5, 10, { offset: 1 })
  assert.deepEqual(r, { start: 4, end: 8 })
})

test('整段平移撞到左端時停住，長度仍然不變（不會被壓扁）', () => {
  const r = applyDrag('move', SEL, 0, 10, { offset: 1 })
  assert.deepEqual(r, { start: 0, end: 4 })
})

test('整段平移撞到右端時停住，長度仍然不變', () => {
  const r = applyDrag('move', SEL, 99, 10, { offset: 1 })
  assert.deepEqual(r, { start: 6, end: 10 })
})

test('xToSeconds 把畫布 x 換成秒', () => {
  assert.equal(xToSeconds(50, 100, 10), 5)
})

test('xToSeconds 夾在畫布兩端（拖出畫布外不會算出負秒或超長）', () => {
  assert.equal(xToSeconds(-30, 100, 10), 0)
  assert.equal(xToSeconds(300, 100, 10), 10)
})

test('xToSeconds 在寬度為 0 時回 0，不會變 NaN', () => {
  assert.equal(xToSeconds(10, 0, 10), 0)
})
