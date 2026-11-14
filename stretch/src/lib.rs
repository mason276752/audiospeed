//! audiospeed 的 WASM 介面：把 [`stretch::stretch`] 包成 C ABI，讓 JS 不靠 wasm-bindgen
//! 就能直接呼叫（介面只有幾個函式，多一套綁定產生器不划算）。
//!
//! 流程：JS 用 [`alloc`] 配置 `channels × len` 個 f32、把樣本**平面排列**寫進去（聲道 0 的
//! 全部樣本、接著聲道 1……）→ [`stretch_planar`] 回傳一個結果代號 → 用 [`result_channel`]
//! 逐聲道讀出 → [`free_result`]、[`dealloc`] 釋放。
//!
//! 結果按聲道交回、不再拼成一整塊：瀏覽器的 WASM 記憶體很吃緊，長音檔放慢時輸出本身
//! 就是輸入的好幾倍，再複製一份拼接就可能配置失敗。

pub mod stretch;

/// 配置 `len` 個 f32（初值 0），給 JS 寫入輸入樣本。用完以同一個 `len` 呼叫 [`dealloc`]。
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut f32 {
    Box::into_raw(vec![0f32; len].into_boxed_slice()) as *mut f32
}

/// 釋放 [`alloc`] 配置的空間。
///
/// # Safety
/// `ptr` 必須來自 [`alloc`]，且 `len` 與配置時相同。
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut f32, len: usize) {
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
}

/// 伸縮後每聲道的樣本數——與 [`stretch::stretch`] 內部同一個公式，JS 不必自己算。
#[no_mangle]
pub extern "C" fn stretched_len(len: usize, ratio: f64) -> usize {
    (len as f64 * ratio).round() as usize
}

/// 時間伸縮。`input` 是 `channels × len` 的平面樣本，`ratio` 是輸出／輸入時長比
/// （速度的倒數）。回傳結果代號，交給 [`result_channel`] 與 [`free_result`]。
///
/// # Safety
/// `input` 必須指向至少 `channels × len` 個有效的 f32。
#[no_mangle]
pub unsafe extern "C" fn stretch_planar(input: *const f32, channels: usize, len: usize, rate: u32, ratio: f64) -> *mut Vec<Vec<f32>> {
    let out = if channels == 0 || len == 0 {
        vec![Vec::new(); channels]
    } else {
        let data = std::slice::from_raw_parts(input, channels * len);
        let chans: Vec<&[f32]> = data.chunks_exact(len).collect();
        stretch::stretch(&chans, rate, ratio)
    };
    Box::into_raw(Box::new(out))
}

/// 結果第 `channel` 聲道的起點（長度是 [`stretched_len`]）。
///
/// # Safety
/// `result` 必須來自 [`stretch_planar`] 且尚未 [`free_result`]。
#[no_mangle]
pub unsafe extern "C" fn result_channel(result: *const Vec<Vec<f32>>, channel: usize) -> *const f32 {
    (&*result)[channel].as_ptr()
}

/// 釋放 [`stretch_planar`] 的結果。
///
/// # Safety
/// `result` 必須來自 [`stretch_planar`]，且只能釋放一次。
#[no_mangle]
pub unsafe extern "C" fn free_result(result: *mut Vec<Vec<f32>>) {
    drop(Box::from_raw(result));
}
