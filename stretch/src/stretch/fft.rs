//! stretch::fft —— radix-2 複數 FFT（就地）與極座標的正/逆轉換。
//!
//! **為什麼不共用既有的那一支**：`ai-voice` 裡有一份更完整的（含 Bluestein，見
//! `indextts::dsp`），但它住在帶著整套 candle 與模型權重的 crate 裡。時間伸縮只需要
//! 40 行的蝶形，而且視窗長度**一定是 2 的冪**（`round_up` 保證），為了它把 `audio`
//! feature 綁上一顆推論引擎不划算。
//!
//! 精度一律 f64：相位推進是逐格累加的，f32 的誤差會沿著整段音訊累積成可聽見的相位漂移。

/// 迭代式 radix-2 複數 FFT（就地，`n = re.len()` 必為 2 的冪）。
///
/// `inverse` 只改旋轉因子的符號，**不**除以 n（縮放由呼叫端決定，貼合參考實作
/// 「在逆轉換前先縮放 magnitude」的做法）。
fn fft_inplace(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    // bit-reversal 重排
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = (i.reverse_bits() >> (usize::BITS - bits)) & (n - 1);
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * 2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        let half = len / 2;
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..half {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + half] * cr - im[i + k + half] * ci,
                    re[i + k + half] * ci + im[i + k + half] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + half] = ur - vr;
                im[i + k + half] = ui - vi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// 實數輸入 → 前 `n/2+1` 個 bin 的（大小, 相位）。`mag`／`phase` 長度須為 `n/2+1`。
///
/// 這裡老實地做整段複數 FFT 再丟掉共軛的那一半（實數 FFT 可以省一半），因為時間伸縮
/// 的成本大頭在「每格都要做一次正轉換＋一次逆轉換」，省這一半換不到體感，卻要多一份
/// 容易寫錯的打包/解包程式。
pub(super) fn forward_polar(x: &[f64], mag: &mut [f64], phase: &mut [f64], scratch: &mut Scratch) {
    let n = x.len();
    scratch.re.clear();
    scratch.re.extend_from_slice(x);
    scratch.im.clear();
    scratch.im.resize(n, 0.0);
    fft_inplace(&mut scratch.re, &mut scratch.im, false);
    for i in 0..=n / 2 {
        let (r, m) = (scratch.re[i], scratch.im[i]);
        mag[i] = (r * r + m * m).sqrt();
        phase[i] = m.atan2(r);
    }
}

/// （大小, 相位）→ 長度 `n` 的實數訊號（**未**除以 n）。
///
/// 以共軛對稱補回上半頻譜——少了這一步逆轉換會吐出複數，取實部等於把訊號的能量砍半
/// 並混進鏡像成分。
pub(super) fn inverse_polar(mag: &[f64], phase: &[f64], out: &mut [f64], scratch: &mut Scratch) {
    let n = out.len();
    let half = n / 2;
    scratch.re.clear();
    scratch.re.resize(n, 0.0);
    scratch.im.clear();
    scratch.im.resize(n, 0.0);
    for i in 0..=half {
        let (s, c) = phase[i].sin_cos();
        scratch.re[i] = mag[i] * c;
        scratch.im[i] = mag[i] * s;
        if i > 0 && i < half {
            scratch.re[n - i] = scratch.re[i];
            scratch.im[n - i] = -scratch.im[i];
        }
    }
    fft_inplace(&mut scratch.re, &mut scratch.im, true);
    out.copy_from_slice(&scratch.re);
}

/// FFT 的暫存緩衝。每格都要轉換兩次，一格配一次 `Vec` 是純粹的浪費，故由呼叫端持有。
#[derive(Default)]
pub(super) struct Scratch {
    re: Vec<f64>,
    im: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正轉換要對得上 DFT 的定義——蝶形寫錯（旋轉因子符號、bit-reversal）不會壞得很明顯，
    /// 只會讓相位整組偏掉，而那在音訊上聽起來只是「怪怪的」。
    #[test]
    fn forward_matches_the_naive_dft() {
        let n = 32;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.7).sin() + 0.3 * (i as f64 * 2.1).cos()).collect();
        let mut mag = vec![0.0; n / 2 + 1];
        let mut phase = vec![0.0; n / 2 + 1];
        forward_polar(&x, &mut mag, &mut phase, &mut Scratch::default());

        for (k, want_mag) in mag.iter().enumerate() {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, v) in x.iter().enumerate() {
                let ang = -2.0 * std::f64::consts::PI * (k * i) as f64 / n as f64;
                re += v * ang.cos();
                im += v * ang.sin();
            }
            assert!((want_mag - (re * re + im * im).sqrt()).abs() < 1e-9, "bin {k} 大小不合");
            assert!((phase[k] - im.atan2(re)).abs() < 1e-9, "bin {k} 相位不合");
        }
    }

    /// 正→逆要原樣還原（除以 n 之後）。
    #[test]
    fn inverse_round_trips_back_to_the_input() {
        let n = 64;
        let x: Vec<f64> = (0..n).map(|i| ((i * i) as f64 * 0.13).sin()).collect();
        let mut mag = vec![0.0; n / 2 + 1];
        let mut phase = vec![0.0; n / 2 + 1];
        let mut scratch = Scratch::default();
        forward_polar(&x, &mut mag, &mut phase, &mut scratch);
        for m in mag.iter_mut() {
            *m /= n as f64;
        }
        let mut out = vec![0.0; n];
        inverse_polar(&mag, &phase, &mut out, &mut scratch);
        for (i, (got, want)) in out.iter().zip(&x).enumerate() {
            assert!((got - want).abs() < 1e-9, "第 {i} 個樣本還原不回來：{got} vs {want}");
        }
    }
}
