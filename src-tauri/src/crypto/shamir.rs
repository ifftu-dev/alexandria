//! Shamir Secret Sharing over GF(256).
//!
//! Pure arithmetic, no external deps. Used by the Sentinel holdout
//! infrastructure to split an AES key across DAO committee members
//! such that any `threshold` of them can reconstruct it.
//!
//! Byte-wise independent: each byte of the secret is split with an
//! independent random polynomial whose degree is `threshold - 1` and
//! whose constant term is the secret byte. Shares evaluate all
//! polynomials at a non-zero x coordinate.
//!
//! The field is GF(2^8) with the Rijndael irreducible polynomial
//! `x^8 + x^4 + x^3 + x + 1` (0x11B) — the same one AES uses.
//! Log/antilog tables make multiply cheap.

use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ShamirError {
    #[error("threshold must be at least 1")]
    ThresholdTooSmall,
    #[error("threshold ({threshold}) cannot exceed share count ({n})")]
    ThresholdExceedsShares { threshold: usize, n: usize },
    #[error("too many shares requested: Shamir over GF(256) supports at most 255")]
    TooManyShares,
    #[error("shares empty or inconsistent length")]
    InconsistentShares,
    #[error("duplicate or zero x coordinate in share set")]
    BadShareIndices,
}

/// One Shamir share: an x coordinate in 1..=255 plus the same number
/// of bytes as the original secret (each byte is `P_i(x)` where
/// `P_i` is the polynomial for secret byte `i`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    pub x: u8,
    pub y: Vec<u8>,
}

// ---------- GF(256) arithmetic ----------

// Log/antilog tables for GF(256) with the AES irreducible polynomial.
// Generator g=3. `LOG[0]` is undefined (0 has no log); callers must
// guard on zero inputs before indexing.
static LOG: [u8; 256] = gen_log_table();
static EXP: [u8; 256] = gen_exp_table();

const fn gen_exp_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 256 {
        table[i] = x as u8;
        // Multiply x by 3 (the generator) in GF(256).
        let mut y = (x << 1) ^ x;
        if y & 0x100 != 0 {
            y ^= 0x11B;
        }
        x = y;
        i += 1;
    }
    table
}

const fn gen_log_table() -> [u8; 256] {
    let exp = gen_exp_table();
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 255 {
        table[exp[i] as usize] = i as u8;
        i += 1;
    }
    table
}

#[inline]
fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let la = LOG[a as usize] as u16;
    let lb = LOG[b as usize] as u16;
    EXP[((la + lb) % 255) as usize]
}

#[inline]
fn gf_div(a: u8, b: u8) -> u8 {
    // Caller guarantees b != 0.
    if a == 0 {
        return 0;
    }
    let la = LOG[a as usize] as i16;
    let lb = LOG[b as usize] as i16;
    let mut diff = la - lb;
    if diff < 0 {
        diff += 255;
    }
    EXP[diff as usize]
}

// ---------- Public API ----------

/// Split `secret` into `n` shares; any `threshold` of them suffice to
/// reconstruct the original bytes. Randomness comes from `OsRng`; this
/// function is not deterministic by design.
pub fn split(secret: &[u8], threshold: usize, n: usize) -> Result<Vec<Share>, ShamirError> {
    if threshold == 0 {
        return Err(ShamirError::ThresholdTooSmall);
    }
    if threshold > n {
        return Err(ShamirError::ThresholdExceedsShares { threshold, n });
    }
    if n > 255 {
        return Err(ShamirError::TooManyShares);
    }

    let mut shares: Vec<Share> = (1..=n as u8)
        .map(|x| Share {
            x,
            y: Vec::with_capacity(secret.len()),
        })
        .collect();

    // For each secret byte, sample a fresh polynomial of degree
    // (threshold-1) with that byte as the constant term, and evaluate
    // at each share's x.
    for &secret_byte in secret {
        let mut coeffs = vec![0u8; threshold];
        coeffs[0] = secret_byte;
        if threshold > 1 {
            let mut buf = vec![0u8; threshold - 1];
            OsRng.fill_bytes(&mut buf);
            coeffs[1..].copy_from_slice(&buf);
        }

        for share in &mut shares {
            share.y.push(eval_poly(&coeffs, share.x));
        }
    }

    Ok(shares)
}

/// Reconstruct a secret from any `>= threshold` shares. Runs Lagrange
/// interpolation at x=0 for each secret byte.
///
/// Caller is responsible for supplying a share set whose size matches
/// the originally chosen threshold; passing more shares than needed is
/// safe but costs extra multiplications.
pub fn combine(shares: &[Share]) -> Result<Vec<u8>, ShamirError> {
    if shares.is_empty() {
        return Err(ShamirError::InconsistentShares);
    }
    let secret_len = shares[0].y.len();
    for s in shares.iter() {
        if s.y.len() != secret_len {
            return Err(ShamirError::InconsistentShares);
        }
    }

    // Reject zero or duplicate x coords — either kills Lagrange.
    let mut xs: Vec<u8> = shares.iter().map(|s| s.x).collect();
    xs.sort_unstable();
    for w in xs.windows(2) {
        if w[0] == w[1] {
            return Err(ShamirError::BadShareIndices);
        }
    }
    if xs[0] == 0 {
        return Err(ShamirError::BadShareIndices);
    }

    let mut out = Vec::with_capacity(secret_len);
    for byte_idx in 0..secret_len {
        let pairs: Vec<(u8, u8)> = shares.iter().map(|s| (s.x, s.y[byte_idx])).collect();
        out.push(lagrange_at_zero(&pairs));
    }
    Ok(out)
}

// ---------- Internals ----------

fn eval_poly(coeffs: &[u8], x: u8) -> u8 {
    // Horner over GF(256).
    let mut acc = 0u8;
    for &c in coeffs.iter().rev() {
        acc = gf_mul(acc, x) ^ c;
    }
    acc
}

fn lagrange_at_zero(pairs: &[(u8, u8)]) -> u8 {
    // f(0) = Σ y_i * Π (x_j / (x_j XOR x_i))  for j != i
    let mut total = 0u8;
    for (i, &(xi, yi)) in pairs.iter().enumerate() {
        let mut num = 1u8;
        let mut den = 1u8;
        for (j, &(xj, _)) in pairs.iter().enumerate() {
            if i == j {
                continue;
            }
            // Lagrange basis at 0: product of (0 - xj) / (xi - xj).
            // In GF(256) subtraction is XOR, so (0 - xj) = xj and
            // (xi - xj) = xi ^ xj.
            num = gf_mul(num, xj);
            den = gf_mul(den, xi ^ xj);
        }
        total ^= gf_mul(yi, gf_div(num, den));
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gf_tables_invert_each_other() {
        for i in 1..=255u8 {
            let log_i = LOG[i as usize];
            assert_eq!(EXP[log_i as usize], i);
        }
    }

    #[test]
    fn gf_mul_is_commutative() {
        for a in 0u8..=32 {
            for b in 0u8..=32 {
                assert_eq!(gf_mul(a, b), gf_mul(b, a));
            }
        }
    }

    #[test]
    fn gf_mul_has_identity_1() {
        for a in 0u8..=255 {
            assert_eq!(gf_mul(a, 1), a);
        }
    }

    #[test]
    fn gf_div_is_mul_inverse() {
        for a in 1u8..=16 {
            for b in 1u8..=16 {
                let q = gf_div(a, b);
                assert_eq!(gf_mul(q, b), a, "{a}/{b}={q}, but {q}*{b}!={a}");
            }
        }
    }

    #[test]
    fn split_and_combine_roundtrip() {
        let secret = b"this is a 32-byte AES secret!!!!";
        let shares = split(secret, 3, 5).unwrap();
        assert_eq!(shares.len(), 5);
        // Any 3 of the 5 should reconstruct the secret.
        let combo = combine(&shares[..3]).unwrap();
        assert_eq!(combo, secret);
        let combo = combine(&[shares[0].clone(), shares[2].clone(), shares[4].clone()]).unwrap();
        assert_eq!(combo, secret);
    }

    #[test]
    fn below_threshold_recovers_wrong_bytes() {
        // With 1 share of a 2-threshold split, we should get an output
        // (interpolation still runs) but it should not equal the secret
        // — Shamir's information-theoretic guarantee is what we want to
        // see verified here, at least statistically for a fixed input.
        let secret = b"secret";
        let shares = split(secret, 2, 3).unwrap();
        let recovered = combine(&shares[..1]).unwrap();
        assert_ne!(recovered, secret);
    }

    #[test]
    fn rejects_threshold_zero() {
        assert!(matches!(
            split(b"x", 0, 3),
            Err(ShamirError::ThresholdTooSmall)
        ));
    }

    #[test]
    fn rejects_threshold_greater_than_n() {
        assert!(matches!(
            split(b"x", 4, 3),
            Err(ShamirError::ThresholdExceedsShares { threshold: 4, n: 3 })
        ));
    }

    #[test]
    fn rejects_too_many_shares() {
        assert!(matches!(
            split(b"x", 1, 256),
            Err(ShamirError::TooManyShares)
        ));
    }

    #[test]
    fn combine_rejects_duplicate_x() {
        let mut shares = split(b"hello world!", 2, 3).unwrap();
        shares[1].x = shares[0].x;
        assert!(matches!(
            combine(&shares),
            Err(ShamirError::BadShareIndices)
        ));
    }

    #[test]
    fn combine_rejects_zero_x() {
        let mut shares = split(b"hello world!", 2, 3).unwrap();
        shares[0].x = 0;
        assert!(matches!(
            combine(&shares),
            Err(ShamirError::BadShareIndices)
        ));
    }

    #[test]
    fn combine_rejects_inconsistent_lengths() {
        let mut shares = split(b"hello world!", 2, 3).unwrap();
        shares[0].y.push(0);
        assert!(matches!(
            combine(&shares),
            Err(ShamirError::InconsistentShares)
        ));
    }

    #[test]
    fn random_aes_key_roundtrips_at_various_thresholds() {
        for &(t, n) in &[(1usize, 1), (2, 2), (2, 3), (3, 5), (4, 7)] {
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            let shares = split(&key, t, n).unwrap();
            assert_eq!(shares.len(), n);
            let combo = combine(&shares[..t]).unwrap();
            assert_eq!(combo, key, "t={t} n={n} failed");
        }
    }
}
