//! Windowed feature extraction for the paste classifier.
//!
//! Bit-identical to the legacy `src/utils/sentinel/paste-features.ts`
//! and the Python `tools/sentinel-train/featurize.py` — the trained
//! ONNX expects exactly this 12-dim layout. If you change the order or
//! any clamp constant, change all three files in the same commit and
//! retrain the model.

use super::types::KeystrokeEvent;

pub const FEATURE_DIM: usize = 12;

#[derive(Debug, Clone)]
pub struct PasteFeatureInputs<'a> {
    pub keystrokes: &'a [KeystrokeEvent],
    pub paste_event_count: u32,
    pub pasted_char_count: u32,
    pub window_ms: f32,
}

/// Feature ordering (stable — model weights are trained against this
/// exact layout):
///
///   0: mean dwellMs
///   1: std  dwellMs
///   2: mean flightMs
///   3: std  flightMs
///   4: fraction of digraphs with flightMs < 5 (paste-burst rate)
///   5: max consecutive zero/near-zero flight run length, /200
///   6: char rate (chars/sec) over the window, /50
///   7: dwell coefficient of variation (std/mean)
///   8: flight coefficient of variation
///   9: paste-event count (capped at 10) / 10
///  10: pasted-character count (capped at 1000) / 1000
///  11: keystroke buffer length / 200, clamped to [0,1]
pub fn extract_paste_features(input: &PasteFeatureInputs) -> [f32; FEATURE_DIM] {
    let mut out = [0.0_f32; FEATURE_DIM];
    let ks = input.keystrokes;
    if ks.is_empty() {
        out[9] = (input.paste_event_count.min(10) as f32) / 10.0;
        out[10] = (input.pasted_char_count.min(1000) as f32) / 1000.0;
        return out;
    }

    let mut dwells = Vec::with_capacity(ks.len());
    let mut flights = Vec::with_capacity(ks.len());
    for k in ks {
        if k.dwell_ms > 0.0 {
            dwells.push(k.dwell_ms);
        }
        if k.flight_ms > 0.0 {
            flights.push(k.flight_ms);
        }
    }

    let mean_d = mean(&dwells);
    let std_d = std_dev(&dwells, mean_d);
    let mean_f = mean(&flights);
    let std_f = std_dev(&flights, mean_f);

    let mut near_zero = 0_u32;
    let mut max_run = 0_u32;
    let mut cur_run = 0_u32;
    for k in ks {
        if k.flight_ms < 5.0 {
            near_zero += 1;
            cur_run += 1;
            if cur_run > max_run {
                max_run = cur_run;
            }
        } else {
            cur_run = 0;
        }
    }

    let char_rate = if input.window_ms > 0.0 {
        (ks.len() as f32 / input.window_ms) * 1000.0
    } else {
        0.0
    };

    out[0] = mean_d;
    out[1] = std_d;
    out[2] = mean_f;
    out[3] = std_f;
    out[4] = near_zero as f32 / ks.len() as f32;
    out[5] = (max_run.min(200) as f32) / 200.0;
    out[6] = char_rate.min(50.0) / 50.0;
    out[7] = if mean_d > 0.01 { std_d / mean_d } else { 0.0 };
    out[8] = if mean_f > 0.01 { std_f / mean_f } else { 0.0 };
    out[9] = (input.paste_event_count.min(10) as f32) / 10.0;
    out[10] = (input.pasted_char_count.min(1000) as f32) / 1000.0;
    out[11] = (ks.len().min(200) as f32) / 200.0;

    for slot in out.iter_mut() {
        if !slot.is_finite() {
            *slot = 0.0;
        }
    }
    out
}

fn mean(xs: &[f32]) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f32>() / xs.len() as f32
}

fn std_dev(xs: &[f32], m: f32) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    let var: f32 = xs.iter().map(|v| (v - m).powi(2)).sum::<f32>() / xs.len() as f32;
    var.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn human_like(n: usize) -> Vec<KeystrokeEvent> {
        (0..n)
            .map(|i| KeystrokeEvent {
                key: "char".into(),
                dwell_ms: 80.0 + (i % 5) as f32 * 4.0,
                flight_ms: 110.0 + (i % 7) as f32 * 6.0,
            })
            .collect()
    }

    fn paste_burst(n: usize) -> Vec<KeystrokeEvent> {
        (0..n)
            .map(|_| KeystrokeEvent {
                key: "char".into(),
                dwell_ms: 1.0,
                flight_ms: 0.5,
            })
            .collect()
    }

    #[test]
    fn empty_returns_zeros_except_event_counts() {
        let f = extract_paste_features(&PasteFeatureInputs {
            keystrokes: &[],
            paste_event_count: 3,
            pasted_char_count: 200,
            window_ms: 0.0,
        });
        assert_eq!(f[9], 0.3);
        assert_eq!(f[10], 0.2);
        for (i, &val) in f.iter().enumerate() {
            if i != 9 && i != 10 {
                assert_eq!(val, 0.0);
            }
        }
    }

    #[test]
    fn paste_burst_marks_near_zero_flight() {
        let ks = paste_burst(100);
        let f = extract_paste_features(&PasteFeatureInputs {
            keystrokes: &ks,
            paste_event_count: 1,
            pasted_char_count: 100,
            window_ms: 30_000.0,
        });
        assert!(f[4] > 0.9, "near-zero-flight frac = {}", f[4]);
        assert!(f[5] > 0.4, "max run / 200 = {}", f[5]);
    }

    #[test]
    fn human_like_has_low_near_zero_flight() {
        let ks = human_like(100);
        let f = extract_paste_features(&PasteFeatureInputs {
            keystrokes: &ks,
            paste_event_count: 0,
            pasted_char_count: 0,
            window_ms: 30_000.0,
        });
        assert!(f[4] < 0.1);
        assert!(f[5] < 0.05);
    }

    #[test]
    fn deterministic() {
        let ks = human_like(40);
        let a = extract_paste_features(&PasteFeatureInputs {
            keystrokes: &ks,
            paste_event_count: 2,
            pasted_char_count: 10,
            window_ms: 25_000.0,
        });
        let b = extract_paste_features(&PasteFeatureInputs {
            keystrokes: &ks,
            paste_event_count: 2,
            pasted_char_count: 10,
            window_ms: 25_000.0,
        });
        for i in 0..FEATURE_DIM {
            assert_eq!(a[i], b[i]);
        }
    }

    #[test]
    fn caps_paste_counts() {
        let f = extract_paste_features(&PasteFeatureInputs {
            keystrokes: &[],
            paste_event_count: 9999,
            pasted_char_count: 9999,
            window_ms: 10_000.0,
        });
        assert!(f[9] <= 1.0);
        assert!(f[10] <= 1.0);
    }
}
