/**
 * 變速的純計算測試。
 *
 * 這裡每一條都對應一種「存出來的檔案不對，但過程沒有任何錯誤訊息」的情況。
 */
import { test } from 'vitest'
import assert from 'node:assert/strict'
import { MAX_SPEED, MIN_SPEED, clampSpeed, fmtSeconds, isUnitSpeed, outputSeconds, suggestName } from './speed.js'

test('速度夾在可用範圍內', () => {
  assert.equal(clampSpeed(10), MAX_SPEED)
  assert.equal(clampSpeed(0), MIN_SPEED)
  assert.equal(clampSpeed(-3), MIN_SPEED)
  assert.equal(clampSpeed('1.5'), 1.5)
  assert.equal(clampSpeed('abc'), 1, '打不出數字時當作不變速，不是當作 0')
  assert.equal(clampSpeed(undefined), 1)
})

test('「不變速」用容差判斷——浮點數的 1.0000000001 也算不變速', () => {
  assert.equal(isUnitSpeed(1), true)
  assert.equal(isUnitSpeed(1 + 1e-12), true)
  assert.equal(isUnitSpeed(1.25), false)
})

test('輸出長度是「選取長度 ÷ 速度」', () => {
  assert.equal(outputSeconds(60, 2), 30)
  assert.equal(outputSeconds(60, 0.5), 120)
  assert.equal(outputSeconds(60, 1), 60)
  assert.equal(outputSeconds(-5, 1), 0, '負的選取長度不該算出負的輸出')
})

test('建議檔名一定不同於來源——合併回 ZIP 時才不會蓋掉原始素材', () => {
  assert.equal(suggestName('旁白.mp3', { trimmed: true }), '旁白-剪輯.wav')
  assert.equal(suggestName('旁白.mp3', { speed: 1.5 }), '旁白-1.5x.wav')
  assert.equal(suggestName('旁白.mp3', { speed: 1.5, trimmed: true }), '旁白-剪輯-1.5x.wav')
  assert.equal(suggestName('旁白.wav', {}), '旁白-副本.wav', '什麼都沒改也不能同名')
  assert.equal(suggestName('素材/旁白.mp3', { trimmed: true }), '旁白-剪輯.wav')
  assert.equal(suggestName('', { trimmed: true }), '音檔-剪輯.wav')
})

test('秒數顯示：一分鐘以內給小數、超過改成 分:秒（跟波形上的格式一致）', () => {
  assert.equal(fmtSeconds(3.14), '3.1 秒')
  assert.equal(fmtSeconds(59.9), '59.9 秒')
  assert.equal(fmtSeconds(60), '1:00')
  assert.equal(fmtSeconds(125), '2:05')
  assert.equal(fmtSeconds(-1), '0.0 秒')
  assert.equal(fmtSeconds(undefined), '0.0 秒')
})
