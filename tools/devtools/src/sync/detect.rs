pub const TONE_HZ: f64 = 1000.0;
pub const GOERTZEL_WIN_S: f64 = 0.010;
pub const GOERTZEL_MIC_WIN_S: f64 = 0.030;
pub const GOERTZEL_HOP_S: f64 = 0.002;
pub const TONE_SNR: f64 = 4.0;
pub const TONE_MIN_AMP: f64 = 1e-3;
pub const FLASH_MIN_STEP: f64 = 20.0;
pub const CAMERA_SIGMAS: f64 = 5.0;
pub const CAMERA_ONSET_FRAC: f64 = 0.1;
pub const CAMERA_MIN_STEP: f64 = 0.1;

pub fn goertzel_series(x: &[f32], rate: u32, freq: f64, win: usize, hop: usize) -> Vec<f64> {
    if win == 0 || hop == 0 || x.len() < win {
        return Vec::new();
    }
    let w = 2.0 * std::f64::consts::PI * freq / f64::from(rate);
    let coeff = 2.0 * w.cos();
    let n = (x.len() - win) / hop + 1;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &v in &x[k * hop..k * hop + win] {
            let s0 = f64::from(v) + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0);
        out.push(2.0 * power.sqrt() / win as f64);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneOnset {
    pub t_ns: i64,
    pub peak: f64,
    pub noise: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Found<T> {
    Yes(T),
    No { peak: f64, noise: f64 },
    NoData,
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

/// The onset is the halfway crossing plus half a window: a window half-filled by the tone reads
/// half its amplitude.
pub fn tone_onset(
    series: &[f64],
    hop_ns: i64,
    win_ns: i64,
    noise_from_ns: i64,
    from_ns: i64,
    to_ns: i64,
) -> Found<ToneOnset> {
    let idx = |t: i64| -> usize { usize::try_from(t.max(0) / hop_ns.max(1)).unwrap_or(0) };
    let (a, b) = (idx(from_ns), idx(to_ns).min(series.len()));
    if a >= b {
        return Found::NoData;
    }
    let mut pre: Vec<f64> = series[idx(noise_from_ns).min(a)..a].to_vec();
    let noise = median(&mut pre).unwrap_or(0.0);
    let (pk_i, peak) = series[a..b]
        .iter()
        .enumerate()
        .fold((0, f64::MIN), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
    if peak < TONE_MIN_AMP || peak < noise * TONE_SNR {
        return Found::No { peak, noise };
    }
    let thr = noise + 0.5 * (peak - noise);
    let mut k = a;
    while k < a + pk_i && series[k] < thr {
        k += 1;
    }
    let pos = if k > a && series[k] > series[k - 1] {
        k as f64 - 1.0 + (thr - series[k - 1]) / (series[k] - series[k - 1])
    } else {
        k as f64
    };
    let t_ns = (pos * hop_ns as f64).round() as i64 + win_ns / 2;
    Found::Yes(ToneOnset { t_ns, peak, noise })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepOnset {
    pub t_ns: i64,
    pub baseline: f64,
    pub peak: f64,
    pub noise_sd: f64,
}

impl StepOnset {
    pub fn step(&self) -> f64 {
        self.peak - self.baseline
    }
}

/// Screen capture sends frames only on change, so an empty baseline range falls back to the
/// last frame before `from_ns`. The onset is the first sample `frac` of the way up the step: the
/// camera's luma ramps over a few hundred ms, so it uses a small `frac` to catch the first rise.
pub fn step_onset(
    series: &[(i64, f64)],
    base_from_ns: i64,
    from_ns: i64,
    to_ns: i64,
    min_step: f64,
    sigmas: f64,
    frac: f64,
) -> Found<StepOnset> {
    let a = series.partition_point(|s| s.0 < from_ns);
    let b = series.partition_point(|s| s.0 < to_ns);
    if a >= b {
        return Found::NoData;
    }
    let base_a = series.partition_point(|s| s.0 < base_from_ns);
    let mut base: Vec<f64> = series[base_a..a].iter().map(|s| s.1).collect();
    if base.is_empty() {
        base.push(if a > 0 { series[a - 1].1 } else { series[a].1 });
    }
    let n = base.len() as f64;
    let mean = base.iter().sum::<f64>() / n;
    let sd = if base.len() > 1 {
        (base.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
    } else {
        0.0
    };
    let baseline = median(&mut base).unwrap_or(mean);
    let peak = series[a..b].iter().map(|s| s.1).fold(f64::MIN, f64::max);
    let step = peak - baseline;
    if step < min_step || step < sigmas * sd {
        return Found::No {
            peak: step,
            noise: sd,
        };
    }
    let thr = baseline + (frac * step).max(3.0 * sd);
    let t_ns = series[a..b]
        .iter()
        .find(|s| s.1 >= thr)
        .map_or(series[a].0, |s| s.0);
    Found::Yes(StepOnset {
        t_ns,
        baseline,
        peak,
        noise_sd: sd,
    })
}

pub fn rms_blocks(x: &[f32], block: usize) -> Vec<f64> {
    if block == 0 {
        return Vec::new();
    }
    x.chunks_exact(block)
        .map(|c| (c.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / block as f64).sqrt())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transient {
    pub onset_block: usize,
    pub peak_block: usize,
    pub peak: f64,
    pub floor: f64,
}

pub fn loudest_transient(env: &[f64], from: usize, to: usize, back: usize) -> Option<Transient> {
    let to = to.min(env.len());
    if from >= to {
        return None;
    }
    let (pk, peak) = env[from..to]
        .iter()
        .enumerate()
        .fold((0, f64::MIN), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
    let pk = from + pk;
    let mut onset = pk;
    while onset > from && pk - onset < back && env[onset - 1] >= 0.5 * peak {
        onset -= 1;
    }
    let mut all = env[from..to].to_vec();
    let floor = median(&mut all).unwrap_or(0.0);
    Some(Transient {
        onset_block: onset,
        peak_block: pk,
        peak,
        floor,
    })
}

pub fn motion_energy(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum::<u64>() as f64
        / a.len() as f64
}

pub fn stats(values: &[f64]) -> Option<(f64, f64, f64)> {
    let mut v = values.to_vec();
    let med = median(&mut v)?;
    Some((med, v[0], v[v.len() - 1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone_at(len_s: f64, at_s: f64, dur_s: f64, amp: f32, noise: f32) -> Vec<f32> {
        let n = (len_s * f64::from(RATE)) as usize;
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        (0..n)
            .map(|i| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let r = (seed >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0;
                let t = i as f64 / f64::from(RATE);
                let s = if t >= at_s && t < at_s + dur_s {
                    amp * (2.0 * std::f64::consts::PI * TONE_HZ * (t - at_s)).sin() as f32
                } else {
                    0.0
                };
                s + noise * r
            })
            .collect()
    }

    fn series(x: &[f32]) -> (Vec<f64>, i64, i64) {
        series_w(x, GOERTZEL_WIN_S)
    }

    fn series_w(x: &[f32], win_s: f64) -> (Vec<f64>, i64, i64) {
        let win = (win_s * f64::from(RATE)) as usize;
        let hop = (GOERTZEL_HOP_S * f64::from(RATE)) as usize;
        (
            goertzel_series(x, RATE, TONE_HZ, win, hop),
            (GOERTZEL_HOP_S * 1e9) as i64,
            (win_s * 1e9) as i64,
        )
    }

    #[test]
    fn goertzel_full_scale_sine_reads_one() {
        let x = tone_at(0.1, 0.0, 1.0, 1.0, 0.0);
        let (s, _, _) = series(&x);
        assert!((s[10] - 1.0).abs() < 0.02, "{}", s[10]);
        let off = goertzel_series(&x, RATE, 3000.0, 480, 96);
        assert!(off[10] < 0.02, "{}", off[10]);
    }

    #[test]
    fn tone_onset_within_a_millisecond() {
        for (at, win_s) in [1.0, 1.0003, 1.0011, 1.0057]
            .into_iter()
            .flat_map(|a| [(a, GOERTZEL_WIN_S), (a, GOERTZEL_MIC_WIN_S)])
        {
            let x = tone_at(2.0, at, 0.1, 0.2, 0.01);
            let (s, hop, win) = series_w(&x, win_s);
            let Found::Yes(o) = tone_onset(&s, hop, win, 500_000_000, 800_000_000, 1_600_000_000)
            else {
                panic!("no onset for {at}");
            };
            let err_ms = (o.t_ns as f64 / 1e6) - at * 1e3;
            assert!(err_ms.abs() <= 1.0, "at {at}: error {err_ms} ms");
            assert!(o.peak > 0.15 && o.noise < 0.01, "{o:?}");
        }
    }

    #[test]
    fn tone_onset_rejects_noise_only() {
        let x = tone_at(2.0, 5.0, 0.1, 0.2, 0.05);
        let (s, hop, win) = series(&x);
        assert!(matches!(
            tone_onset(&s, hop, win, 500_000_000, 800_000_000, 1_600_000_000),
            Found::No { .. }
        ));
        assert_eq!(
            tone_onset(&s, hop, win, 0, 5_000_000_000, 6_000_000_000),
            Found::NoData
        );
    }

    #[test]
    fn step_onset_uses_held_frame_and_halfway_point() {
        let s = vec![
            (0, 16.0),
            (1_000_000_000, 16.0),
            (2_010_000_000, 130.0),
            (2_043_000_000, 235.0),
            (2_110_000_000, 16.0),
        ];
        let Found::Yes(o) = step_onset(
            &s,
            1_500_000_000,
            1_700_000_000,
            2_800_000_000,
            20.0,
            0.0,
            0.5,
        ) else {
            panic!("no step");
        };
        assert_eq!(o.t_ns, 2_010_000_000);
        assert_eq!(o.baseline, 16.0);
        assert_eq!(o.peak, 235.0);
    }

    #[test]
    fn step_onset_gates_on_noise() {
        let mut s: Vec<(i64, f64)> = (0..60)
            .map(|i| (i * 33_000_000, if i % 2 == 0 { 50.0 } else { 54.0 }))
            .collect();
        s.push((2_000_000_000, 57.0));
        let r = step_onset(&s, 0, 1_990_000_000, 2_500_000_000, 1.5, 5.0, 0.5);
        assert!(matches!(r, Found::No { .. }), "{r:?}");
        s.push((2_033_000_000, 90.0));
        let Found::Yes(o) = step_onset(&s, 0, 1_990_000_000, 2_500_000_000, 1.5, 5.0, 0.5) else {
            panic!("no step");
        };
        assert_eq!(o.t_ns, 2_033_000_000);
        assert!(o.noise_sd > 1.0 && o.step() > 30.0, "{o:?}");
    }

    #[test]
    fn step_onset_catches_the_first_rise_of_a_ramp() {
        let mut s: Vec<(i64, f64)> = (0..30)
            .map(|i| (i * 33_000_000, 18.70 + 0.01 * (i % 3) as f64))
            .collect();
        for (k, v) in [18.74, 18.78, 18.82, 18.85, 18.89, 18.92, 18.94]
            .iter()
            .enumerate()
        {
            s.push((1_000_000_000 + k as i64 * 33_000_000, *v));
        }
        let Found::Yes(o) = step_onset(&s, 0, 950_000_000, 2_000_000_000, 0.1, 5.0, 0.1) else {
            panic!("no step");
        };
        assert_eq!(o.t_ns, 1_000_000_000);
        let Found::Yes(half) = step_onset(&s, 0, 950_000_000, 2_000_000_000, 0.1, 5.0, 0.5) else {
            panic!("no step");
        };
        assert_eq!(half.t_ns, 1_099_000_000);
    }

    #[test]
    fn clap_transient_and_motion() {
        let mut env = vec![0.01; 1000];
        for (i, v) in [0.2, 0.6, 0.9, 0.5, 0.2].iter().enumerate() {
            env[500 + i] = *v;
        }
        env[100] = 0.3;
        let t = loudest_transient(&env, 0, 1000, 25).expect("transient");
        assert_eq!((t.onset_block, t.peak_block), (501, 502));
        assert!((t.floor - 0.01).abs() < 1e-9);
        assert_eq!(motion_energy(&[10, 20, 30], &[10, 25, 20]), 5.0);
        assert_eq!(motion_energy(&[1], &[1, 2]), 0.0);
        assert_eq!(stats(&[3.0, 1.0, 2.0]), Some((2.0, 1.0, 3.0)));
    }
}
