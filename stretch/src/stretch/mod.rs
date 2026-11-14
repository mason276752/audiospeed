//! stretch —— 純 Rust 的時間伸縮（音檔加快／變慢，**音高不變**）。
//!
//! 移植自 rubberband 的 R2（"faster"）引擎、**離線模式**、只保留時間伸縮那一條路：
//! 不做移調、不做共振峰保持、不做即時串流、不做多執行緒。參考實作在
//! `referance/rubberband/src/faster/`（`R2Stretcher` / `StretcherProcess` /
//! `StretchCalculator`）。
//!
//! # 為什麼不是「直接改播放速率」
//!
//! 重採樣（改播放速率）會連音高一起改——語音變成花栗鼠或大叔。要維持音高就得在頻域做：
//! 把訊號切成重疊的短格做 FFT，**輸出時用不同的步長重疊回去**，同時把每個 bin 的相位
//! 依新步長重新推進。這就是相位聲碼器。
//!
//! # 三個部分，缺一個就會很難聽
//!
//! | 部分 | 檔案 | 少了它會怎樣 |
//! |---|---|---|
//! | 起音偵測 | [`onset`] | 每個子音／鼓點被拉成「咻——」的一團 |
//! | 逐格計畫表 | [`plan`] | 拉伸平均攤到每一格，瞬態跟著一起被拉長 |
//! | 相位鎖定（laminar） | [`modify_phases`] | 泛音各自漂移，聲音變得空洞、有金屬味 |
//!
//! # 精度與效能
//!
//! 全程 f64（相位是逐格累加的，f32 誤差會沿著整段累積）。成本大約是每聲道每格兩次
//! 2048 點 FFT——一分鐘 48kHz 單聲道約十萬次乘加級距的工作量，實測遠快於即時，
//! 所以端點是同步算完就回（不進背景任務佇列）。

mod fft;
mod onset;
mod plan;

#[cfg(test)]
mod tests;

/// 相位鎖定時，一個 bin 最多可以跟著上一個 bin「漂移」幾格。
const MAX_INHERIT_DISTANCE: f64 = 8.0;

/// 分析／合成的尺寸（離線模式；分析與合成視窗同長，故只有一個 `window`）。
struct Sizes {
    /// FFT 長度＝視窗長度（2 的冪）。
    window: usize,
    /// 輸入端的固定步長。輸出端的步長逐格不同，見 [`plan`]。
    increment: usize,
}

impl Sizes {
    /// 依取樣率與伸縮比決定視窗與步長（移植自 `R2Stretcher::calculateSizes` 的離線分支）。
    ///
    /// 兩個方向的取捨相反：**變快**（ratio<1）時輸入端要密一點（步長小）才不會漏掉細節；
    /// **變慢**（ratio>1）時是輸出端要密一點，所以改從輸出步長回推輸入步長。
    fn plan(rate: u32, ratio: f64, input_len: usize) -> Sizes {
        // 視窗長度跟著取樣率縮放（48kHz 用 2048），不然高取樣率的音檔等於用了更短的視窗。
        let base_fft = round_up((2048.0 * (rate as f64 / 48000.0)) as usize);
        let mut window = base_fft;
        let mut increment;

        if ratio < 1.0 {
            increment = window / 4;
            while increment >= 512 {
                increment /= 2;
            }
            if (increment as f64 * ratio).floor() < 1.0 {
                // 壓縮到連一個樣本都不到（ratio < 1/256）：反過來由輸出步長 1 回推。
                increment = round_up((1.0 / ratio).ceil() as usize);
                window = round_up(increment * 4);
            }
        } else {
            let mut out_inc = window / 6;
            increment = (out_inc as f64 / ratio) as usize;
            // 輸出步長太大會讓重疊不足（聽得到一格一格的接縫），對半砍到 1024 以內。
            while out_inc > 1024 && increment > 1 {
                out_inc /= 2;
                increment = (out_inc as f64 / ratio) as usize;
            }
            while increment < 1 {
                out_inc *= 2;
                increment = (out_inc as f64 / ratio) as usize;
            }
            window = window.max(round_up(out_inc * 6));
            if ratio > 5.0 {
                while window < 8192 {
                    window *= 2;
                }
            }
        }

        // 音檔很短時把步長縮小，至少切得出幾格來（不然整段只有一格，計畫表無事可做）。
        while increment * 4 > input_len && increment > 1 {
            increment /= 2;
        }
        Sizes { window, increment: increment.max(1) }
    }
}

/// 無條件進位到 2 的冪（FFT 只吃 2 的冪）。
fn round_up(v: usize) -> usize {
    v.max(1).next_power_of_two()
}

/// 週期型 Hann 窗。分析與合成用同一個窗，重疊相加後除以窗的累積量，增益才會回到 1。
fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
        .collect()
}

/// 把相位摺回 (-π, π]。
fn princarg(a: f64) -> f64 {
    a - 2.0 * std::f64::consts::PI * (a / (2.0 * std::f64::consts::PI)).round()
}

/// 對多聲道 PCM 做時間伸縮。
///
/// * `channels` —— 每聲道一條 f32 PCM（值域約 [-1,1]），長度須一致
/// * `ratio` —— 輸出/輸入時長比。`0.5` ＝ 速度兩倍（變快），`2.0` ＝ 速度一半（變慢）
///
/// 回傳同樣的聲道數，每條長度 `round(輸入長度 × ratio)`。
///
/// **所有聲道共用同一份計畫表**（由各聲道混成單聲道後分析得出）。各聲道各自偵測起音的話，
/// 左右兩邊會在不同的格子做相位重置，立體聲像會在每個鼓點上晃一下。
///
/// 聲道吃 `AsRef<[f32]>`，WASM 那層才能直接傳 JS 寫好的緩衝切片進來，不必先複製成 `Vec`。
pub fn stretch<C: AsRef<[f32]>>(channels: &[C], rate: u32, ratio: f64) -> Vec<Vec<f32>> {
    let input_len = channels.first().map(|c| c.as_ref().len()).unwrap_or(0);
    let out_len = (input_len as f64 * ratio).round() as usize;
    if input_len == 0 || channels.is_empty() {
        return channels.iter().map(|_| Vec::new()).collect();
    }

    let sizes = Sizes::plan(rate, ratio, input_len);
    let mono = mix_down(channels, input_len);
    let (df, silence) = study(&mono, &sizes, rate);

    let mut increments = plan::calculate(ratio, &df, sizes.increment, rate);
    plan::mark_silence_resets(&mut increments, &silence, sizes.window, sizes.increment);

    channels
        .iter()
        .map(|ch| synthesise(ch.as_ref(), &increments, &sizes, rate, ratio, out_len))
        .collect()
}

/// 各聲道相加後除以聲道數（分析用；輸出仍然是各聲道獨立處理的）。
fn mix_down<C: AsRef<[f32]>>(channels: &[C], len: usize) -> Vec<f32> {
    if channels.len() == 1 {
        return channels[0].as_ref().to_vec();
    }
    let mut mono = vec![0.0f32; len];
    for ch in channels {
        for (m, v) in mono.iter_mut().zip(ch.as_ref().iter()) {
            *m += *v;
        }
    }
    let scale = 1.0 / channels.len() as f32;
    for m in mono.iter_mut() {
        *m *= scale;
    }
    mono
}

/// 取第 `chunk` 格的輸入樣本，套上分析窗。
///
/// 輸入前面墊了半個視窗的靜音（`window/2`），第一格才會**以第一個樣本為中心**——
/// 少了這個墊片，開頭那半個視窗會被窗函數的上升緣吃掉，聽起來像淡入。
fn read_windowed(input: &[f32], chunk: usize, sizes: &Sizes, win: &[f64], out: &mut [f64]) {
    let pad = sizes.window / 2;
    let start = chunk * sizes.increment;
    for (i, o) in out.iter_mut().enumerate() {
        let pos = start + i;
        let s = if pos < pad { 0.0 } else { input.get(pos - pad).copied().unwrap_or(0.0) };
        *o = s as f64 * win[i];
    }
}

/// 掃過整段音訊，算出每一格的起音函數值與「是不是靜音」。
fn study(mono: &[f32], sizes: &Sizes, rate: u32) -> (Vec<f32>, Vec<bool>) {
    let win = hann(sizes.window);
    let last_bin = onset::last_perceived_bin(rate, sizes.window);
    let mut curve = onset::OnsetCurve::new(rate, sizes.window);
    let mut scratch = fft::Scratch::default();
    let mut frame = vec![0.0f64; sizes.window];
    let mut mag = vec![0.0f64; sizes.window / 2 + 1];
    let mut phase = vec![0.0f64; sizes.window / 2 + 1];

    let total = sizes.window / 2 + mono.len();
    let mut df = Vec::new();
    let mut silence = Vec::new();
    let mut chunk = 0usize;
    // 離線模式一路做到底：只要還剩半個視窗的資料就再切一格（不足的部分補零）。
    while total.saturating_sub(chunk * sizes.increment) >= sizes.window / 2 {
        read_windowed(mono, chunk, sizes, &win, &mut frame);
        // 只要大小、不要相位，所以這裡不必做 fftshift（省下來的是每格一次搬移）。
        fft::forward_polar(&frame, &mut mag, &mut phase, &mut scratch);
        df.push(curve.process(&mag));
        silence.push(onset::is_silent(&mag, last_bin));
        chunk += 1;
    }
    (df, silence)
}

/// 一個聲道跨格延續的相位狀態。
struct PhaseState {
    prev_phase: Vec<f64>,
    prev_error: Vec<f64>,
    unwrapped: Vec<f64>,
}

/// 相位鎖定的頻段界線（bin 索引）。低頻要鎖得緊、高頻可以放。
struct Limits {
    /// 這個 bin 以下完全不繼承（每個 bin 各自推進）。
    limit0: usize,
    limit1: usize,
    limit2: usize,
}

impl Limits {
    fn plan(rate: u32, fft_size: usize, ratio: f64) -> Limits {
        let (mut f0, mut f1, mut f2) = (600.0f64, 1200.0f64, 12000.0f64);
        if ratio > 1.0 {
            // 拉得越慢，越要把「允許跟隨」的下限往上推：慢速下低頻的相位誤差最刺耳。
            let rf0 = 600.0 + 600.0 * ((ratio - 1.0).powi(3) * 2.0);
            let (r1, r2) = (f1 / f0, f2 / f0);
            f0 = f0.max(rf0);
            f1 = f0 * r1;
            f2 = f0 * r2;
        }
        let bin = |f: f64| ((f * fft_size as f64) / rate as f64).round() as usize;
        let limit0 = bin(f0);
        let limit1 = bin(f1).max(limit0);
        let limit2 = bin(f2).max(limit1);
        Limits { limit0, limit1, limit2 }
    }

    /// 這個 bin 最多允許離「峰值」幾格還繼續繼承相位。
    fn max_distance(&self, bin: usize) -> f64 {
        if bin <= self.limit0 {
            0.0
        } else if bin <= self.limit1 {
            1.0
        } else if bin <= self.limit2 {
            3.0
        } else {
            MAX_INHERIT_DISTANCE
        }
    }
}

/// 逐 bin 重新推進相位（移植自 `R2Stretcher::modifyChunk`，laminar 模式）。
///
/// 核心是一行：`advance = 輸出步長 × ((omega + 相位誤差) / 輸入步長)`——把「這個 bin 在
/// 輸入步長內實際走了多少相位」換算成新步長該走多少。
///
/// 難的是相鄰 bin 的一致性。同一個泛音會佔好幾個 bin，各自獨立推進的話它們會慢慢散開
/// （聲音變空洞、有金屬味）。這裡用參考實作的 laminar 判準：相位誤差的**不穩定度**與
/// **變化方向**都跟上一個 bin 一致時，就認定它們屬於同一條泛音，讓這個 bin 去跟隨上一個
/// bin 的相位推進量；跟隨得越遠（distance 越大）就越拉回自己算的值。
fn modify_phases(
    st: &mut PhaseState,
    phase: &mut [f64],
    sizes: &Sizes,
    limits: &Limits,
    out_inc: usize,
    reset: bool,
) {
    let count = sizes.window / 2;
    let mut prev_instability = 0.0f64;
    let mut prev_direction = false;
    let mut distance = 0.0f64;

    for i in (0..=count).rev() {
        let p = phase[i];
        let mut perr = 0.0;
        let mut outphase = p;

        if reset {
            // 相位重置：原封不動地用分析相位（瞬態的波形因此原樣保留）。
            distance = 0.0;
        } else {
            let omega = (2.0 * std::f64::consts::PI * sizes.increment as f64 * i as f64) / sizes.window as f64;
            let expected = st.prev_phase[i] + omega;
            perr = princarg(p - expected);

            let instability = (perr - st.prev_error[i]).abs();
            let direction = perr > st.prev_error[i];
            let mi = limits.max_distance(i);
            let inherit = !(distance >= mi || i == count)
                && instability > prev_instability
                && direction == prev_direction;

            let mut advance = out_inc as f64 * ((omega + perr) / sizes.increment as f64);
            if inherit {
                let inherited = st.unwrapped[i + 1] - st.prev_phase[i + 1];
                advance = (advance * distance + inherited * (MAX_INHERIT_DISTANCE - distance)) / MAX_INHERIT_DISTANCE;
                outphase = p + advance;
                distance += 1.0;
            } else {
                outphase = st.unwrapped[i] + advance;
                distance = 0.0;
            }
            prev_instability = instability;
            prev_direction = direction;
        }

        st.prev_error[i] = perr;
        st.prev_phase[i] = p;
        st.unwrapped[i] = outphase;
        phase[i] = outphase;
    }
}

/// 單一聲道的分析→修相位→合成→重疊相加。
fn synthesise(
    input: &[f32],
    increments: &[i64],
    sizes: &Sizes,
    rate: u32,
    ratio: f64,
    out_len: usize,
) -> Vec<f32> {
    let n = sizes.window;
    let half = n / 2;
    let win = hann(n);
    // 合成端每格都要把窗本身累加起來，最後拿輸出去除它——輸出步長逐格不同，重疊量因此
    // 也逐格不同，只有這樣增益才會處處為 1（固定的常數會在步長變化處造成音量起伏）。
    let win_scale = 0.75; // 分析窗的面積（Hann＝0.5）× 1.5，照搬參考實作
    let limits = Limits::plan(rate, n, ratio);

    let mut st = PhaseState {
        prev_phase: vec![0.0; half + 1],
        prev_error: vec![0.0; half + 1],
        unwrapped: vec![0.0; half + 1],
    };
    let mut scratch = fft::Scratch::default();
    let mut frame = vec![0.0f64; n];
    let mut shifted = vec![0.0f64; n];
    let mut mag = vec![0.0f64; half + 1];
    let mut phase = vec![0.0f64; half + 1];
    let mut accumulator = vec![0.0f64; n];
    let mut window_accumulator = vec![0.0f64; n];
    // 前面多出的半個視窗＋收尾吐出的一整個視窗都會進來；預留足夠，長音檔才不會在最後
    // 一刻因為擴容而暫時佔用兩倍記憶體（瀏覽器裡的 WASM 記憶體很吃緊）。
    let mut raw: Vec<f32> = Vec::with_capacity(out_len + 2 * n);

    for (k, inc) in increments.iter().enumerate() {
        let phase_increment = inc.unsigned_abs() as usize;
        // 第一格一律當成重置：沒有「上一格」可以推進相位。
        let reset = *inc < 0 || k == 0;
        // 這一格要往前推進多少，看的是**下一格**的增量（參考實作同樣的錯位）：
        // 增量描述的是「這一格的相位推進」，而位移要等下一格決定它從哪裡開始。
        let shift = increments.get(k + 1).map(|v| v.unsigned_abs() as usize).unwrap_or(phase_increment);

        read_windowed(input, k, sizes, &win, &mut frame);
        // fftshift：把視窗中心搬到樣本 0，FFT 的相位才是以視窗中心為基準（相位推進的前提）。
        shifted[..half].copy_from_slice(&frame[half..]);
        shifted[half..].copy_from_slice(&frame[..half]);
        fft::forward_polar(&shifted, &mut mag, &mut phase, &mut scratch);
        modify_phases(&mut st, &mut phase, sizes, &limits, phase_increment, reset);

        // 逆轉換前先縮放（參考實作如此，避免定點 FFT 溢位；這裡是習慣延續）
        for m in mag.iter_mut() {
            *m /= n as f64;
        }
        fft::inverse_polar(&mag, &phase, &mut shifted, &mut scratch);
        frame[..half].copy_from_slice(&shifted[half..]);
        frame[half..].copy_from_slice(&shifted[..half]);
        for i in 0..n {
            accumulator[i] += frame[i] * win[i];
            window_accumulator[i] += win[i] * win_scale;
        }

        emit(&mut raw, &mut accumulator, &mut window_accumulator, shift.min(n));
    }

    // 收尾：把累加器裡剩下的重疊全部吐出來（尾巴會被下面的長度裁切修掉）。
    emit(&mut raw, &mut accumulator, &mut window_accumulator, n);

    // 第一格是以第一個樣本為中心算的，所以輸出前面多了半個視窗；裁掉它，長度才對得上。
    // 就地裁切，不另外複製一份。
    let skip = half.min(raw.len());
    raw.drain(..skip);
    raw.resize(out_len, 0.0);
    raw
}

/// 把累加器最前面 `count` 個樣本除以窗累加量後吐出去，並把兩個累加器往前移。
fn emit(out: &mut Vec<f32>, accumulator: &mut [f64], window_accumulator: &mut [f64], count: usize) {
    for i in 0..count {
        // 窗累加量趨近 0 的地方沒有任何一格覆蓋到（只會出現在最尾巴），除下去會炸成雜訊。
        let w = window_accumulator[i];
        out.push(if w > 1e-6 { (accumulator[i] / w) as f32 } else { 0.0 });
    }
    accumulator.copy_within(count.., 0);
    window_accumulator.copy_within(count.., 0);
    let keep = accumulator.len() - count;
    accumulator[keep..].fill(0.0);
    window_accumulator[keep..].fill(0.0);
}
