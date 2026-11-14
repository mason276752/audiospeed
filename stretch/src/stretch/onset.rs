//! stretch::onset —— 起音偵測函數（onset detection function）。
//!
//! 移植自 rubberband R2 的 `CompoundAudioCurve`（＝`PercussiveAudioCurve` ＋
//! `HighFrequencyAudioCurve` ＋兩個移動百分位濾波器）與 `SilentAudioCurve`。
//!
//! **這是整套時間伸縮裡最容易被誤刪的一段**：拿掉它音檔還是會變快變慢，只是每個
//! 子音、每一下鼓點都會被拉成「咻——」的一團（相位聲碼器的招牌瑕疵）。它算出來的
//! 曲線交給 [`super::plan`] 找出轉折點，那些點上改成整段搬移、不做相位內插，
//! 瞬態才留得住。
//!
//! 所有輸入都是**未正規化**的 FFT 大小（沒有除以 N），與參考實作一致——裡面的
//! 絕對門檻（`1e-8`、`1e-6`）是照那個尺度定的。

use std::collections::VecDeque;

/// 人耳有意義的最高頻率：超過這裡的 bin 不看（參考實作的 `MaxPerceivedFreq`）。
const MAX_PERCEIVED_FREQ: usize = 16000;

/// 起音偵測：一格頻譜 → 一個 df 值。狀態跨格延續，故是個 struct 而不是純函式。
pub(super) struct OnsetCurve {
    /// 只看到這個 bin 為止（16kHz 以上對「哪裡是起音」沒有貢獻，只有雜訊）。
    last_bin: usize,
    /// 上一格的頻譜大小（打擊型偵測要比較兩格之間的成長）。
    prev_mag: Vec<f64>,
    hf_filter: MovingPercentile,
    hf_deriv_filter: MovingPercentile,
    last_hf: f64,
    last_result: f64,
    rising_count: u32,
}

impl OnsetCurve {
    pub(super) fn new(rate: u32, fft_size: usize) -> Self {
        let last_bin = ((MAX_PERCEIVED_FREQ * fft_size) / rate.max(1) as usize).min(fft_size / 2);
        Self {
            last_bin,
            prev_mag: vec![0.0; fft_size / 2 + 1],
            // 19 格窗、85／90 百分位：不是中位數——要的是「明顯高於近期水準」，
            // 取中位數會讓一般的音量起伏也算成起音。
            hf_filter: MovingPercentile::new(19, 85.0),
            hf_deriv_filter: MovingPercentile::new(19, 90.0),
            last_hf: 0.0,
            last_result: 0.0,
            rising_count: 0,
        }
    }

    /// 餵進一格頻譜大小，取回這一格的 df（0 ~ 1 左右，越大越像起音）。
    pub(super) fn process(&mut self, mag: &[f64]) -> f32 {
        let percussive = self.percussive(mag);
        let hf = self.high_frequency(mag);
        self.filter(percussive, hf) as f32
    }

    /// 打擊型偵測：有多少比例的 bin 在這一格突然變大 3dB 以上。
    fn percussive(&mut self, mag: &[f64]) -> f64 {
        // 10^0.15 ＝ 能量平方成長 3dB
        const THRESHOLD: f64 = 1.4125375446227544;
        const ZERO_THRESH: f64 = 1e-8;
        let (mut count, mut non_zero) = (0usize, 0usize);
        // bin 0（直流）不看：它沒有「音高」可言，而任何直流偏移都會讓它一直算成有能量。
        for (now, prev) in mag[1..=self.last_bin].iter().zip(&self.prev_mag[1..=self.last_bin]) {
            let v = if *prev > ZERO_THRESH {
                now / prev
            } else if *now > ZERO_THRESH {
                THRESHOLD
            } else {
                0.0
            };
            if v >= THRESHOLD {
                count += 1;
            }
            if *now > ZERO_THRESH {
                non_zero += 1;
            }
        }
        self.prev_mag[..=self.last_bin].copy_from_slice(&mag[..=self.last_bin]);
        if non_zero == 0 {
            0.0
        } else {
            count as f64 / non_zero as f64
        }
    }

    /// 高頻含量（bin 序號加權的總和）：子音與弦音的起音會先在高頻冒出來。
    fn high_frequency(&self, mag: &[f64]) -> f64 {
        mag[..=self.last_bin].iter().enumerate().map(|(n, m)| m * n as f64).sum()
    }

    /// 把兩條曲線合成一個 df。
    ///
    /// 高頻那條看的是「超出近期水準」的**導數轉折**：一路上升然後開始下降的那一格，
    /// 才記一筆 0.5——峰值本身而不是整段上升都算成起音。打擊型那條夠強（>0.35）時直接蓋過去。
    fn filter(&mut self, percussive: f64, hf: f64) -> f64 {
        let mut rv = 0.0;
        let hf_deriv = hf - self.last_hf;

        self.hf_filter.push(hf);
        self.hf_deriv_filter.push(hf_deriv);
        let hf_filtered = self.hf_filter.get();
        let hf_deriv_filtered = self.hf_deriv_filter.get();
        self.last_hf = hf;

        let mut result = 0.0;
        if hf - hf_filtered > 0.0 {
            result = hf_deriv - hf_deriv_filtered;
        }

        if result < self.last_result {
            if self.rising_count > 3 && self.last_result > 0.0 {
                rv = 0.5;
            }
            self.rising_count = 0;
        } else {
            self.rising_count += 1;
        }

        if percussive > 0.35 && percussive > rv {
            rv = percussive;
        }
        self.last_result = result;
        rv
    }
}

/// 這一格是不是「完全沒有聲音」。
///
/// 靜音區段連續夠久就強制相位重置（見 [`super::plan::mark_silence_resets`]）：那裡沒有
/// 要保護的相位連續性，重置反而讓下一個真正的起音乾淨地開始。
pub(super) fn is_silent(mag: &[f64], last_bin: usize) -> bool {
    const THRESHOLD: f64 = 1e-6;
    !mag[..=last_bin].iter().any(|&m| m > THRESHOLD)
}

/// 對應 `OnsetCurve` 的 `last_bin`（`is_silent` 是自由函式，要由呼叫端算好同一個值）。
pub(super) fn last_perceived_bin(rate: u32, fft_size: usize) -> usize {
    ((MAX_PERCEIVED_FREQ * fft_size) / rate.max(1) as usize).min(fft_size / 2)
}

/// 移動視窗百分位濾波器（移植自 `MovingMedian`，只是我們一定用非 50 的百分位）。
///
/// 語意照抄參考實作：視窗**還沒填滿時就開始給值**（前幾格的百分位是在較少的樣本上取的），
/// 且不置中——取的是「最近 N 格」而不是「前後各 N/2 格」。這兩件事都會影響曲線的形狀，
/// 改了就對不上 rubberband 的偵測結果。
struct MovingPercentile {
    window: VecDeque<f64>,
    capacity: usize,
    percentile: f64,
    sorted: Vec<f64>,
}

impl MovingPercentile {
    fn new(capacity: usize, percentile: f64) -> Self {
        Self { window: VecDeque::with_capacity(capacity), capacity, percentile, sorted: Vec::new() }
    }

    fn push(&mut self, v: f64) {
        if self.window.len() == self.capacity {
            self.window.pop_front();
        }
        self.window.push_back(if v.is_nan() { 0.0 } else { v });
    }

    fn get(&mut self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        self.sorted.clear();
        self.sorted.extend(self.window.iter().copied());
        self.sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = self.sorted.len() - 1;
        let index = ((n as f64 * self.percentile / 100.0).floor() as usize).min(n);
        self.sorted[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 百分位是在「已推入的樣本數」上算的，不是視窗容量——填滿前後的取值規則要一致。
    #[test]
    fn the_percentile_filter_indexes_by_how_many_values_it_has_seen() {
        let mut f = MovingPercentile::new(5, 90.0);
        f.push(1.0);
        assert_eq!(f.get(), 1.0, "只有一個值時，任何百分位都是它自己");
        for v in [2.0, 3.0, 4.0, 5.0] {
            f.push(v);
        }
        // n = 4，floor(4 * 0.9) = 3 → 第 4 小的值
        assert_eq!(f.get(), 4.0);
        f.push(6.0); // 擠掉 1.0
        assert_eq!(f.get(), 5.0);
    }

    /// 一段能量突然整體變大的頻譜要被認成起音；平穩的不該被認成起音。
    #[test]
    fn a_sudden_broadband_rise_reads_as_an_onset() {
        let fft = 512;
        let mut curve = OnsetCurve::new(48000, fft);
        let bins = fft / 2 + 1;
        let steady = vec![1.0f64; bins];
        let mut quiet_df = 0.0f32;
        for _ in 0..10 {
            quiet_df = curve.process(&steady);
        }
        let loud = vec![10.0f64; bins];
        let onset_df = curve.process(&loud);
        assert!(quiet_df < 0.1, "平穩訊號不該有起音：{quiet_df}");
        assert!(onset_df > 0.9, "整段抬升 20dB 應該幾乎所有 bin 都算數：{onset_df}");
    }

    /// 全零頻譜算靜音；有一個 bin 超過門檻就不算。
    #[test]
    fn silence_needs_every_audible_bin_below_the_floor() {
        let mag = vec![0.0f64; 129];
        assert!(is_silent(&mag, 128));
        let mut mag2 = mag.clone();
        mag2[7] = 1e-5;
        assert!(!is_silent(&mag2, 128));
        // 超過可聽 bin 之外的能量不影響判斷
        let mut mag3 = mag.clone();
        mag3[120] = 1.0;
        assert!(is_silent(&mag3, 64));
    }
}
