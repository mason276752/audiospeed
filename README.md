# 音檔變速

在瀏覽器裡把音檔加快或放慢，**音高不變**，也能剪出其中一段。純前端，檔案不會上傳到任何伺服器。

👉 https://mason276752.github.io/audiospeed/

## 功能

- **變速不變調**：0.25×～4×，Podcast、課程錄音加快聽，語言教材放慢聽
- **剪出一段**：在波形上拖曳選取範圍（拖兩側把手調整、拖中間整段移動），可以先播放確認
- **批次處理**：一次放好幾個檔，用同一個速度整批轉好
- **輸出 WAV**：保留原本的聲道數（16-bit PCM）；多個結果可以打包下載
- **格式**：MP3、M4A、WAV、FLAC、OGG……瀏覽器播得了的都解得開（用內建的 `decodeAudioData`）

## 和 ZIP 編輯器串接

工具都放在 `mason276752.github.io` 底下，屬於同一個 origin，可以用 [BroadcastChannel](https://developer.mozilla.org/docs/Web/API/BroadcastChannel) 互傳檔案：

- 在 [ZIP 編輯器](https://mason276752.github.io/zipeditor/) 勾選音檔（或在預覽裡）按「音檔變速」，檔案會傳過來（`{type:'files'}`）
- 這裡按「傳到 ZIP 編輯器」，結果打包成 ZIP 傳回去（`{type:'zip'}`），可以合併到目前的資料夾

對方的分頁沒開的話會自動開新分頁。協定見 [`src/bridge.js`](src/bridge.js)，與 zipeditor、paste2jpg 相同。

## 變速核心

`stretch/` 是 [Rubber Band](https://breakfastquay.com/rubberband/) R2（"faster"）引擎離線模式的純 Rust 移植，只保留時間伸縮：起音偵測、逐格計畫表、相位鎖定（laminar）的相位聲碼器。沒有任何依賴，編成約 70KB 的 WASM，在 Web Worker 裡跑（介面見 `stretch/src/lib.rs`，C ABI，不用 wasm-bindgen）。

原本是 rust-ai-webtool（私人專案） 後端的一部分；波形、選取、WAV 的前端程式也一起搬了過來。

## 開發

需要 Rust（含 `wasm32-unknown-unknown` target）與 Node。

```sh
rustup target add wasm32-unknown-unknown
npm install
npm run build:wasm   # 編 stretch/ → src/stretch/stretch.wasm
npm run dev          # http://localhost:5173/audiospeed/
npm test             # 前端（vitest）＋ Rust（cargo test）
npm run build        # 輸出到 dist/
```

推到 `main` 後，GitHub Actions 會編 WASM、跑測試、建置並發布到 GitHub Pages。

## 授權

因為變速核心移植自 Rubber Band（GPL-2.0-or-later），整個專案以 **GPL-2.0-or-later** 授權，全文見 [`LICENSE`](LICENSE)。前端使用 [Vue](https://vuejs.org/)（MIT）。
