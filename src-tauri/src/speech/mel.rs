//! Whisper's mel filter bank, computed rather than shipped.
//!
//! Whisper builds its log-mel input with `librosa.filters.mel(sr=16000,
//! n_fft=400, n_mels=…)`: the Slaney mel scale (linear below 1 kHz,
//! logarithmic above) with Slaney area normalisation. candle's
//! `audio::pcm_to_mel` takes that bank as a flat row-major `n_mels × 201`
//! table; this builds the same table.

const SAMPLE_RATE: f64 = 16_000.0;
const N_FFT: usize = 400;
/// FFT bins the bank covers: `1 + N_FFT / 2`.
pub const N_FREQS: usize = 1 + N_FFT / 2;

const F_SP: f64 = 200.0 / 3.0;
const MIN_LOG_HZ: f64 = 1000.0;
const MIN_LOG_MEL: f64 = MIN_LOG_HZ / F_SP;

fn log_step() -> f64 {
    6.4f64.ln() / 27.0
}

fn hz_to_mel(hz: f64) -> f64 {
    if hz >= MIN_LOG_HZ {
        MIN_LOG_MEL + (hz / MIN_LOG_HZ).ln() / log_step()
    } else {
        hz / F_SP
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    if mel >= MIN_LOG_MEL {
        MIN_LOG_HZ * (log_step() * (mel - MIN_LOG_MEL)).exp()
    } else {
        F_SP * mel
    }
}

/// The `n_mels × N_FREQS` filter bank, row-major.
pub fn filters(n_mels: usize) -> Vec<f32> {
    let fft_freqs: Vec<f64> = (0..N_FREQS)
        .map(|i| i as f64 * (SAMPLE_RATE / 2.0) / (N_FREQS - 1) as f64)
        .collect();
    let (mel_min, mel_max) = (hz_to_mel(0.0), hz_to_mel(SAMPLE_RATE / 2.0));
    let mel_f: Vec<f64> = (0..n_mels + 2)
        .map(|i| mel_to_hz(mel_min + (mel_max - mel_min) * i as f64 / (n_mels + 1) as f64))
        .collect();

    let mut bank = vec![0f32; n_mels * N_FREQS];
    for m in 0..n_mels {
        let (left, centre, right) = (mel_f[m], mel_f[m + 1], mel_f[m + 2]);
        let norm = 2.0 / (right - left);
        for (k, &freq) in fft_freqs.iter().enumerate() {
            let lower = (freq - left) / (centre - left);
            let upper = (right - freq) / (right - centre);
            let weight = lower.min(upper).max(0.0);
            bank[m * N_FREQS + k] = (weight * norm) as f32;
        }
    }
    bank
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from candle's `melfilters.bytes` (the bank Whisper ships as
    /// `mel_filters.npz`, 80 mels).
    #[test]
    fn matches_whisper_reference_values() {
        let bank = filters(80);
        assert_eq!(bank.len(), 80 * N_FREQS);
        let reference = [
            (0, 1, 0.024_862_595_f32),
            (0, 2, 0.0),
            (1, 2, 0.022_871_772),
            (79, 186, 0.000_366_741_67),
            (79, 190, 0.002_232_055),
            (79, 193, 0.003_141_313_3),
            (79, 199, 0.000_448_759),
            (79, 200, 0.0),
        ];
        for (m, k, expected) in reference {
            let got = bank[m * N_FREQS + k];
            assert!(
                (got - expected).abs() < 1e-6,
                "mel {m} bin {k}: got {got}, expected {expected}"
            );
        }
        let row0_nonzero = (0..N_FREQS).filter(|&k| bank[k] != 0.0).count();
        assert_eq!(row0_nonzero, 1);
    }

    /// Compares every value against candle's reference table when it is on
    /// disk: `SPEECH_MELFILTERS=/path/melfilters.bytes cargo test … -- --ignored`.
    #[test]
    #[ignore = "needs candle's melfilters.bytes on disk"]
    fn matches_whisper_reference_table() {
        let path = std::env::var("SPEECH_MELFILTERS").expect("SPEECH_MELFILTERS");
        let bytes = std::fs::read(path).expect("read");
        let reference: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let bank = filters(80);
        assert_eq!(bank.len(), reference.len());
        let worst = bank
            .iter()
            .zip(&reference)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst < 1e-6, "max abs difference {worst}");
    }
}
