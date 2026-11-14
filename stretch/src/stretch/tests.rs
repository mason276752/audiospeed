//! 時間伸縮的驗收測試。
//!
//! 「聽起來對不對」沒辦法在 CI 裡斷言，所以這裡盯的是**錯了就一定聽得出來**的四件事：
//! 長度、音高、響度、瞬態落點。相位聲碼器壞掉的典型症狀（音高跑掉、音量抽動、
//! 起音被抹平）都會在其中一條上現形。

use super::*;

/// 48kHz，測試一律用它——`Sizes::plan` 的視窗基準就是照 48kHz 定的。
const RATE: u32 = 48_000;

/// 產生一段正弦波。
fn sine(freq: f64, seconds: f64, rate: u32) -> Vec<f32> {
    let n = (seconds * rate as f64) as usize;
    (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin() as f32)
        .collect()
}

/// 取中段 8192 個樣本做 FFT，找出能量最強的頻率（Hz）。
fn dominant_freq(x: &[f32], rate: u32) -> f64 {
    const N: usize = 8192;
    assert!(x.len() >= N, "訊號太短，量不出頻率");
    let start = (x.len() - N) / 2;
    let win = hann(N);
    let frame: Vec<f64> = (0..N).map(|i| x[start + i] as f64 * win[i]).collect();
    let mut mag = vec![0.0; N / 2 + 1];
    let mut phase = vec![0.0; N / 2 + 1];
    fft::forward_polar(&frame, &mut mag, &mut phase, &mut fft::Scratch::default());
    let peak = mag
        .iter()
        .enumerate()
        .skip(1)
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0);
    peak as f64 * rate as f64 / N as f64
}

fn rms(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

/// 輸出長度就是「輸入長度 × 比例」——這是使用者唯一會拿碼表去對的東西。
///
/// 素材只用 0.4 秒：長度是逐格分配算出來的，跟素材多長無關（原本用 2 秒，
/// 在 debug build 要 2.1 秒；縮短後 0.4 秒，斷言一字未改）。
#[test]
fn the_output_lasts_exactly_as_long_as_asked() {
    let input = sine(440.0, 0.4, RATE);
    for ratio in [0.5, 0.8, 1.25, 2.0] {
        let out = stretch(std::slice::from_ref(&input), RATE, ratio);
        let want = (input.len() as f64 * ratio).round() as usize;
        assert_eq!(out[0].len(), want, "ratio {ratio} 的長度不對");
    }
}

/// **比例 1.0 要還原出原訊號**。
///
/// 這是整條管線最值錢的一條測試：視窗、fftshift、相位推進、重疊相加的正規化、
/// 頭尾裁切——任何一處寫錯，相關係數都不會是 1。其他測試量的是「結果像不像」，
/// 只有這一條量的是「有沒有算錯」。
#[test]
fn a_ratio_of_one_reconstructs_the_original() {
    // 顫音＋泛音：單一正弦波太寬容（相位就算漂掉也還原得回去）
    let n = RATE as usize;
    let input: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let vib = 220.0 * t + 0.02 * (2.0 * std::f64::consts::PI * 5.0 * t).sin();
            (0.5 * (2.0 * std::f64::consts::PI * vib).sin()
                + 0.25 * (2.0 * std::f64::consts::PI * 2.0 * vib).sin()) as f32
        })
        .collect();
    let out = stretch(std::slice::from_ref(&input), RATE, 1.0);
    assert_eq!(out[0].len(), input.len());
    let (mut num, mut d1, mut d2) = (0.0f64, 0.0, 0.0);
    for (a, b) in input.iter().zip(&out[0]) {
        let (a, b) = (*a as f64, *b as f64);
        num += a * b;
        d1 += a * a;
        d2 += b * b;
    }
    let corr = num / (d1.sqrt() * d2.sqrt());
    assert!(corr > 0.999, "比例 1.0 沒有還原原訊號：相關係數 {corr:.4}");
}

/// 音高不能跟著變——這正是「不用重採樣」的全部理由。
///
/// 對照組就寫在斷言裡：直接改播放速率的話，440Hz 在兩倍速下會變成 880Hz。
#[test]
fn the_pitch_survives_both_directions() {
    // 1.2 秒：量頻率要 8192 個樣本的視窗，而 0.5 倍速會把素材壓成一半，
    // 壓完仍有 0.6 秒（28800 樣本）綽綽有餘。
    let input = sine(440.0, 1.2, RATE);
    for ratio in [0.5, 2.0] {
        let out = stretch(std::slice::from_ref(&input), RATE, ratio);
        let f = dominant_freq(&out[0], RATE);
        assert!(
            (f - 440.0).abs() < 12.0,
            "ratio {ratio} 之後主頻跑到 {f:.1}Hz（重採樣會變成 {:.0}Hz）",
            440.0 / ratio
        );
    }
}

/// 響度要維持——重疊相加的正規化（除以窗累加量）寫錯的話，音量會隨伸縮比整體偏掉，
/// 或在輸出步長變化的地方一抽一抽的。
/// 素材 1 秒就夠：頭尾各避開一個視窗之後仍有大半段可以量（原本 2 秒要 1.6 秒）。
#[test]
fn the_loudness_stays_put() {
    let input = sine(440.0, 1.0, RATE);
    let want = rms(&input);
    for ratio in [0.5, 1.5, 2.5] {
        let out = stretch(std::slice::from_ref(&input), RATE, ratio);
        // 頭尾各半個視窗是淡入淡出的區域，量響度時避開
        let body = &out[0][2048..out[0].len() - 2048];
        let got = rms(body);
        assert!(
            (got / want - 1.0).abs() < 0.15,
            "ratio {ratio} 的響度變成原本的 {:.2} 倍",
            got / want
        );
    }
}

/// 瞬態要落在**時間上正確的位置**，而不是被平均地攤開。
///
/// 這一條顧的是 [`plan`] 那半套：拿掉起音偵測，這個測試仍然會過（位置是等比例的），
/// 但拿掉「起音那一格整格搬移」之後，尖峰會被拉寬——所以下面也一併量了尖峰的寬度。
#[test]
fn a_transient_lands_where_it_should_and_stays_narrow() {
    let rate = RATE;
    let mut input = vec![0.0f32; rate as usize * 2];
    // 一聲短促的爆音（帶點衰減的寬頻脈衝，比單一樣本更接近真實的鼓點）
    let hit = rate as usize; // 第 1.0 秒
    for i in 0..200 {
        let env = (-(i as f64) / 40.0).exp();
        input[hit + i] = (env * ((i as f64 * 0.7).sin() + (i as f64 * 2.3).sin()) * 0.4) as f32;
    }
    let out = stretch(std::slice::from_ref(&input), rate, 2.0);
    let (peak_at, peak) = out[0]
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v.abs()))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();
    let at_sec = peak_at as f64 / rate as f64;
    assert!((at_sec - 2.0).abs() < 0.08, "爆音落在 {at_sec:.3} 秒，應該在 2.0 秒");

    // 尖峰寬度：超過峰值一半的樣本數。原始爆音約 100 個樣本，允許被撐開到 3 倍以內。
    let wide = out[0].iter().filter(|v| v.abs() > peak * 0.5).count();
    assert!(wide < 300, "爆音被拉寬成 {wide} 個樣本，瞬態沒保住");
}

/// 立體聲的兩個聲道要用同一份計畫表，兩邊的長度與瞬態位置才會一致。
#[test]
fn both_channels_are_stretched_by_the_same_plan() {
    let left = sine(440.0, 0.8, RATE);
    let right: Vec<f32> = left.iter().map(|v| v * 0.5).collect();
    let out = stretch(&[left.clone(), right], RATE, 1.7);
    assert_eq!(out[0].len(), out[1].len(), "兩聲道長度不一致");
    // 右聲道是左聲道的一半音量，伸縮後這個關係要維持
    let ratio = rms(&out[1][2048..out[1].len() - 2048]) / rms(&out[0][2048..out[0].len() - 2048]);
    assert!((ratio - 0.5).abs() < 0.05, "兩聲道的音量關係跑掉了：{ratio:.3}");
}

/// 全靜音的輸入要吐出全靜音——窗累加量在無資料處是 0，沒擋好就會除出 NaN／inf，
/// 而那在 WAV 裡是刺耳的雜訊，不是無聲。
#[test]
fn silence_in_silence_out() {
    let input = vec![0.0f32; RATE as usize];
    let out = stretch(std::slice::from_ref(&input), RATE, 2.0);
    assert_eq!(out[0].len(), RATE as usize * 2);
    assert!(out[0].iter().all(|v| v.is_finite() && v.abs() < 1e-6), "靜音伸縮後不是靜音");
}

/// 極短的音檔（比一個視窗還短）不能 panic，長度照樣要對。
#[test]
fn a_clip_shorter_than_one_window_still_works() {
    let input = sine(440.0, 0.01, RATE); // 480 個樣本
    let out = stretch(std::slice::from_ref(&input), RATE, 2.0);
    assert_eq!(out[0].len(), 960);
    assert!(out[0].iter().all(|v| v.is_finite()));
}

/// 視窗與步長的規劃：變快時輸入端要密、變慢時輸出端要密，而且視窗永遠是 2 的冪。
#[test]
fn the_window_plan_follows_the_direction_of_the_stretch() {
    let long = 48_000 * 10;
    let faster = Sizes::plan(RATE, 0.5, long);
    let slower = Sizes::plan(RATE, 2.0, long);
    assert!(faster.window.is_power_of_two() && slower.window.is_power_of_two());
    assert_eq!(faster.window, 2048, "48kHz 的基準視窗是 2048");
    assert!(
        slower.increment < faster.increment,
        "變慢時輸入步長要更小（{} vs {}）",
        slower.increment,
        faster.increment
    );
    // 取樣率越高，視窗跟著放大（不然等於用了更短的時間窗）
    assert_eq!(Sizes::plan(96_000, 0.5, long).window, 4096);
    assert_eq!(Sizes::plan(24_000, 0.5, long).window, 1024);
}
