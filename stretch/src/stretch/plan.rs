//! stretch::plan —— 「哪一格輸出多長」的計畫表（移植自 rubberband 的 `StretchCalculator`）。
//!
//! 相位聲碼器不是每一格都用同樣的輸出步長。整段的平均步長由速度決定，但**起音那一格
//! 要用原始步長整個搬過去**（不拉長也不壓縮，且不做相位內插），拉伸的份量攤給起音之間
//! 的平順段落。這就是鼓點與子音不會被抹成一團的原因。
//!
//! 計畫表是一串「每格的輸出增量」，**負數代表這一格要相位重置**（照搬參考實作的編碼；
//! 用一個平行的 bool 陣列也可以，但那就得兩邊同步，反而更容易走鐘）。

use std::collections::BTreeSet;

/// 偵測到的轉折點。`hard`＝夠劇烈、要做相位重置；`soft`＝只要求「時間上落在正確位置」。
struct Peak {
    chunk: usize,
    hard: bool,
}

/// 算出每一格的輸出增量（負值＝相位重置）。
///
/// * `ratio` —— 輸出/輸入時長比（＞1 變慢、＜1 變快）
/// * `df` —— 每格的起音函數值（見 [`super::onset`]）
/// * `increment` —— 分析步長（輸入端固定）
pub(super) fn calculate(ratio: f64, df: &[f32], increment: usize, rate: u32) -> Vec<i64> {
    let total_count = df.len();
    if total_count == 0 {
        return Vec::new();
    }
    // 輸出總長以「格數×步長×比例」算，而不是輸入樣本數×比例：計畫表是逐格分配的，
    // 用同一個尺度算總量才不會在最後一格出現一個補不齊的零頭。
    let output_duration = ((total_count * increment) as f64 * ratio).round() as usize;

    let peaks = find_peaks(df, rate, increment);
    // 轉折點在輸出時間軸上的位置：等比例對應（不做關鍵影格映射——我們只做等速變速）。
    let targets: Vec<usize> = peaks
        .iter()
        .map(|p| ((p.chunk as f64 * output_duration as f64) / total_count as f64).round() as usize)
        .collect();

    let mut increments: Vec<i64> = Vec::with_capacity(total_count);

    for i in 0..=peaks.len() {
        let (mut region_start_chunk, mut region_start, phase_reset) = if i == 0 {
            (0usize, 0usize, false)
        } else {
            (peaks[i - 1].chunk, targets[i - 1], peaks[i - 1].hard)
        };
        let (mut region_end_chunk, mut region_end) = if i == peaks.len() {
            (total_count, output_duration)
        } else {
            (peaks[i].chunk, targets[i])
        };

        region_start_chunk = region_start_chunk.min(total_count);
        region_start = region_start.min(output_duration);
        region_end_chunk = region_end_chunk.min(total_count).max(region_start_chunk);
        region_end = region_end.min(output_duration).max(region_start);

        let region_duration = region_end - region_start;
        let nchunks = region_end_chunk - region_start_chunk;
        if nchunks == 0 {
            continue;
        }

        let mut per = region_duration as f64 / nchunks as f64;
        let mut acc = 0.0f64;
        let mut nremaining = nchunks;
        let mut total_for_region = 0usize;

        if phase_reset {
            // 起音這一格用原始步長原樣搬過去；剩下的長度攤給這一段的其餘各格。
            let incr = if nchunks > 1 { increment.min(region_duration) } else { region_duration };
            increments.push(-(incr as i64));
            per = if nchunks > 1 {
                (region_duration - incr) as f64 / (nchunks - 1) as f64
            } else {
                0.0
            };
            acc += incr as f64;
            total_for_region += incr;
            nremaining -= 1;
        }

        for j in 0..nremaining {
            if j + 1 < nremaining {
                acc += per;
                // 逐格四捨五入會漂移，故每格都拿「累積目標 - 已配出」的差額，讓誤差不累積。
                let incr = (acc - total_for_region as f64).round().max(0.0) as usize;
                increments.push(incr as i64);
                total_for_region += incr;
            } else if region_duration > total_for_region {
                // 最後一格收尾：把剩下的全給它，總長才會剛好等於 region_duration。
                increments.push((region_duration - total_for_region) as i64);
                total_for_region = region_duration;
            } else {
                // 這一段的長度已經被前面的格子配光（只在 per≈0 的退化區段發生）。
                // **與參考實作刻意不同**：那裡是「不推」，於是計畫表會比格數少一筆，
                // 之後靠處理迴圈的邊界夾擠去兜。這裡推一個 0，維持「一格一筆」的不變量
                // ——合成迴圈是照這個索引走的，少一筆會讓後面每一格都對錯相位重置點。
                increments.push(0);
            }
        }
    }

    increments
}

/// 連續靜音夠久的格子改成相位重置。
///
/// 靜音裡沒有要保護的相位連續性，重置反而讓靜音之後的第一個音乾淨地開始；不重置的話
/// 聲碼器會把靜音前的殘留相位一路帶過去，聽起來像一層甩不掉的金屬味殘響。
pub(super) fn mark_silence_resets(increments: &mut [i64], silence: &[bool], window: usize, increment: usize) {
    let need = (window / increment.max(1)) as i32;
    let mut history = 0i32;
    for (i, inc) in increments.iter_mut().enumerate() {
        match silence.get(i) {
            Some(true) => history += 1,
            Some(false) => history = 0,
            None => break,
        }
        if history >= need && *inc >= 0 {
            *inc = -*inc;
        }
    }
}

/// 找出起音函數上的轉折點。
///
/// 分兩種：**hard**（劇烈上升，通常是寬頻的瞬態）做相位重置，位置取在「開始急升」那一格
/// 而不是峰值，這樣瞬態的形狀才留得住；**soft**（平滑後的峰值挑選）只保證時間位置正確。
fn find_peaks(raw_df: &[f32], rate: u32, increment: usize) -> Vec<Peak> {
    let df = smooth_df(raw_df);
    let mut hard_candidates: BTreeSet<usize> = BTreeSet::new();
    let mut soft_candidates: BTreeSet<usize> = BTreeSet::new();

    // 兩個 hard peak 至少隔 0.05 秒——連續的急升是同一個瞬態的一部分，不是好幾個。
    let hard_amnesty = (rate as f64 / (20.0 * increment as f64)).ceil() as usize;
    let mut prev_hard = 0usize;

    for i in 1..df.len().saturating_sub(1) {
        if df[i] < 0.1 || df[i] <= df[i - 1] * 1.1 || df[i] < 0.22 {
            continue;
        }
        if !hard_candidates.is_empty() && i < prev_hard + hard_amnesty {
            continue;
        }
        // 四條擇一：絕對值夠高／一次急升 40%／連兩次 20%／連三次 10% 且不算低。
        let mut hard = df[i] > 0.4;
        if !hard {
            hard = df[i] > df[i - 1] * 1.4;
        }
        if !hard && i > 1 {
            hard = df[i] > df[i - 1] * 1.2 && df[i - 1] > df[i - 2] * 1.2;
        }
        if !hard && i > 2 {
            hard = df[i] > 0.3 && df[i - 1] > df[i - 2] * 1.1 && df[i - 2] > df[i - 3] * 1.1;
        }
        if !hard {
            continue;
        }
        let mut location = i;
        // 下一格還要再大幅上升的話，真正的起音在下一格。
        if i + 1 < raw_df.len() && raw_df[i + 1] > raw_df[i] * 1.4 {
            location += 1;
        }
        hard_candidates.insert(location);
        prev_hard = location;
    }

    collect_soft_peaks(&df, rate, increment, &mut soft_candidates);
    merge(hard_candidates, soft_candidates)
}

/// 以移動視窗的 90 百分位當門檻挑峰值（視窗約 1 秒，峰值間至少隔 0.05 秒）。
fn collect_soft_peaks(df: &[f32], rate: u32, increment: usize, out: &mut BTreeSet<usize>) {
    let median_max = ((rate as f64 / increment as f64).ceil() as usize).max(7);
    let min_spacing = (rate as f64 / (20.0 * increment as f64)).ceil() as i64;

    // 視窗預先填一半的零：讓「中間那一格」從第 0 格就對得上 df 的第 0 格。
    let mut win: std::collections::VecDeque<f32> = std::collections::VecDeque::new();
    for _ in 0..median_max / 2 {
        win.push_back(0.0);
    }
    for &v in df.iter().take(median_max / 2) {
        win.push_back(v);
    }

    let mut amnesty = 0i64;
    let mut last_soft = usize::MAX;
    let mut sorted: Vec<f32> = Vec::with_capacity(median_max);

    for i in 0..df.len() {
        let median_size = median_max.min(win.len());
        let middle = (median_max / 2).min(median_size.saturating_sub(1));
        let next_df = i + median_size - middle;

        // 視窗還太短（音檔比視窗還短時會發生），這一格沒有可信的門檻可用。
        // 參考實作在這裡會讀到視窗外（C++ 不檢查），Rust 會 panic，所以明確跳過。
        if median_size >= 2 && middle >= 1 && middle + 1 < win.len() {
            sorted.clear();
            sorted.extend(win.iter().take(median_size).copied());
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mut index = (sorted.len() * 90) / 100;
            if index >= sorted.len() {
                index = sorted.len() - 1;
            }
            if index == sorted.len() - 1 && index > 0 {
                index -= 1;
            }
            let thresh = sorted[index];

            if win[middle] > thresh && win[middle] > win[middle - 1] && win[middle] > win[middle + 1] && amnesty == 0 {
                // 峰頂可能還在後面幾格（門檻是在中間那格判的），往後找到真正的最高點。
                let mut max_index = middle;
                let mut max_val = win[middle];
                for j in middle + 1..median_size {
                    if win[j] > max_val {
                        max_val = win[j];
                        max_index = j;
                    } else if win[j] < win[middle] {
                        break;
                    }
                }
                let peak = i + max_index - middle;
                if peak < df.len() && last_soft != peak {
                    out.insert(peak);
                    last_soft = peak;
                }
                amnesty = min_spacing + (max_index as i64 - middle as i64);
            } else if amnesty > 0 {
                amnesty -= 1;
            }
        } else if amnesty > 0 {
            amnesty -= 1;
        }

        if median_size >= median_max {
            win.pop_front();
        }
        win.push_back(df.get(next_df).copied().unwrap_or(0.0));
    }
}

/// 兩組候選合成一條依時間排序的轉折點清單。
///
/// hard 優先：hard peak 之後 3 格內的 soft peak 是同一個瞬態的餘波，丟掉——不然
/// 一個鼓點會被切成「重置一次、又固定一次」，中間那一小段被擠得沒有時間可以拉伸。
fn merge(hard: BTreeSet<usize>, soft: BTreeSet<usize>) -> Vec<Peak> {
    let mut hard: Vec<usize> = hard.into_iter().collect();
    let mut soft: Vec<usize> = soft.into_iter().collect();
    hard.reverse(); // 當成 stack 用，pop 取最小
    soft.reverse();
    let mut peaks: Vec<Peak> = Vec::new();

    while !hard.is_empty() || !soft.is_empty() {
        let take_hard = match (hard.last(), soft.last()) {
            (Some(h), Some(s)) => h <= s,
            (Some(_), None) => true,
            _ => false,
        };
        if take_hard {
            peaks.push(Peak { chunk: hard.pop().unwrap(), hard: true });
        } else {
            let chunk = soft.pop().unwrap();
            let shadowed = peaks.last().is_some_and(|p| p.hard && p.chunk + 3 >= chunk);
            if !shadowed {
                peaks.push(Peak { chunk, hard: false });
            }
        }
    }
    peaks
}

/// 三格移動平均：df 本身很毛躁，直接挑峰值會挑到雜訊。
fn smooth_df(df: &[f32]) -> Vec<f32> {
    (0..df.len())
        .map(|i| {
            let mut total = df[i];
            let mut count = 1.0f32;
            if i > 0 {
                total += df[i - 1];
                count += 1.0;
            }
            if i + 1 < df.len() {
                total += df[i + 1];
                count += 1.0;
            }
            total / count
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 計畫表配出來的輸出總長要等於「輸入時長×比例」，而且格數不多不少。
    /// 這是整個伸縮最基本的保證——差幾個樣本聽不出來，差幾格就會對不上原本的節奏。
    #[test]
    fn the_plan_adds_up_to_the_requested_duration() {
        for ratio in [0.5, 0.75, 1.5, 2.0, 3.3] {
            let df = vec![0.02f32; 400];
            let increments = calculate(ratio, &df, 256, 48000);
            assert_eq!(increments.len(), df.len(), "每一格都要有一個增量（ratio {ratio}）");
            let total: i64 = increments.iter().map(|v| v.abs()).sum();
            let want = ((400 * 256) as f64 * ratio).round() as i64;
            assert_eq!(total, want, "ratio {ratio} 的輸出總長不對");
        }
    }

    /// 一個明顯的起音要變成 hard peak，而那一格的增量是負的（＝相位重置）、
    /// 大小就是原始步長：瞬態原樣搬過去，拉伸的份量由鄰近的格子吸收。
    #[test]
    fn a_transient_becomes_a_phase_reset_that_keeps_its_original_increment() {
        let mut df = vec![0.02f32; 200];
        for v in df.iter_mut().skip(100).take(3) {
            *v = 0.9;
        }
        let increments = calculate(2.0, &df, 256, 48000);
        let resets: Vec<usize> = increments
            .iter()
            .enumerate()
            .filter(|(_, v)| **v < 0)
            .map(|(i, _)| i)
            .collect();
        assert!(!resets.is_empty(), "整段只有一個起音，卻一個相位重置都沒有");
        assert!(
            resets.iter().any(|&i| (99..=103).contains(&i)),
            "相位重置沒有落在起音附近：{resets:?}"
        );
        for &i in &resets {
            assert_eq!(increments[i], -256, "重置的那一格要用原始步長整格搬移");
        }
        // 總長仍然要對得上（重置格吃掉的長度由其他格補回來）
        let total: i64 = increments.iter().map(|v| v.abs()).sum();
        assert_eq!(total, ((200 * 256) as f64 * 2.0).round() as i64);
    }

    /// 連續靜音超過一個視窗長度就轉成相位重置；零星的靜音格不算。
    #[test]
    fn a_long_silence_flips_its_increments_to_resets() {
        let mut increments = vec![100i64; 20];
        let mut silence = vec![false; 20];
        silence[3] = true; // 孤立一格，不夠久
        for s in silence.iter_mut().skip(10).take(9) {
            *s = true;
        }
        mark_silence_resets(&mut increments, &silence, 1024, 256); // 需要連續 4 格
        assert_eq!(increments[3], 100, "只有一格靜音不該重置");
        assert_eq!(increments[12], 100, "第 3 格連續靜音還不夠");
        assert_eq!(increments[13], -100, "連續第 4 格起開始重置");
        assert_eq!(increments[18], -100);
    }

    /// 音檔短到連一個中位數視窗都填不滿時不能爆掉（參考實作在這裡會讀到界外）。
    #[test]
    fn a_clip_shorter_than_the_median_window_still_produces_a_plan() {
        let df = vec![0.5f32, 0.1, 0.9];
        let increments = calculate(1.5, &df, 256, 48000);
        assert_eq!(increments.len(), 3);
        let total: i64 = increments.iter().map(|v| v.abs()).sum();
        assert_eq!(total, ((3 * 256) as f64 * 1.5).round() as i64);
    }
}
