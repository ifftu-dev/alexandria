use std::io::Read;

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use flate2::{bufread::GzDecoder, Decompress, FlushDecompress, Status};
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde::Serialize;
use zeroize::{Zeroize, Zeroizing};

const MAX_COMPRESSED: usize = 2048;
const PADDED_SIZE: usize = 1536;
const MAX_SIGNED: usize = PADDED_SIZE - 9;
const MAX_DECODED: usize = MAX_SIGNED + 256;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InputError {
    #[error("invalid or oversized QR encoding")]
    Encoding,
    #[error("unsupported QR structure")]
    Structure,
    #[error("QR signature does not match the supplied RSA key")]
    Signature,
    #[error("invalid circuit binding")]
    Binding,
}

type Result<T> = std::result::Result<T, InputError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedInput {
    qr_data_padded: Vec<String>,
    qr_data_padded_length: String,
    delimiter_indices: Vec<String>,
    signature: Vec<String>,
    pub_key: Vec<String>,
    nullifier_seed: String,
    signal_hash: String,
    reveal_age_above18: String,
    reveal_gender: String,
    reveal_pin_code: String,
    reveal_state: String,
    #[serde(skip)]
    exact_timestamp: u64,
}

impl Drop for PreparedInput {
    fn drop(&mut self) {
        self.qr_data_padded.zeroize();
        self.qr_data_padded_length.zeroize();
        self.delimiter_indices.zeroize();
        self.signature.zeroize();
        self.pub_key.zeroize();
        self.nullifier_seed.zeroize();
        self.signal_hash.zeroize();
        self.exact_timestamp.zeroize();
    }
}

impl PreparedInput {
    pub fn exact_timestamp(&self) -> u64 {
        self.exact_timestamp
    }

    pub fn to_json(&self) -> Result<Zeroizing<Vec<u8>>> {
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|_| InputError::Encoding)
    }
}

fn decimal_bytes(decimal: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if decimal.is_empty()
        || decimal.len() > 4933
        || decimal[0] == b'0'
        || !decimal.iter().all(u8::is_ascii_digit)
    {
        return Err(InputError::Encoding);
    }
    let mut bytes = Zeroizing::new(vec![0_u8; MAX_COMPRESSED]);
    let mut used = 1;
    for digit in decimal {
        let mut carry = u16::from(digit - b'0');
        for byte in &mut bytes[..used] {
            let value = u16::from(*byte) * 10 + carry;
            *byte = value as u8;
            carry = value >> 8;
        }
        if carry != 0 {
            if used == bytes.len() {
                return Err(InputError::Encoding);
            }
            bytes[used] = carry as u8;
            used += 1;
        }
    }
    bytes.truncate(used);
    bytes.reverse();
    Ok(bytes)
}

fn decompress(bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let mut output = Zeroizing::new(vec![0; MAX_DECODED + 1]);
    let (count, remaining) = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(bytes);
        let count = read_bounded(&mut decoder, &mut output)?;
        (count, decoder.into_inner())
    } else {
        let mut decoder = Decompress::new(true);
        let status = decoder
            .decompress(bytes, &mut output, FlushDecompress::Finish)
            .map_err(|_| InputError::Encoding)?;
        if status != Status::StreamEnd {
            return Err(InputError::Encoding);
        }
        (
            decoder.total_out() as usize,
            &bytes[decoder.total_in() as usize..],
        )
    };
    if count <= 256 || count > MAX_DECODED || !remaining.is_empty() {
        return Err(InputError::Encoding);
    }
    output.truncate(count);
    Ok(output)
}

fn read_bounded(reader: &mut impl Read, output: &mut [u8]) -> Result<usize> {
    let mut count = 0;
    while count < output.len() {
        let read = reader
            .read(&mut output[count..])
            .map_err(|_| InputError::Encoding)?;
        if read == 0 {
            return Ok(count);
        }
        count += read;
    }
    Err(InputError::Encoding)
}

fn structure(data: &[u8]) -> Result<([usize; 18], u64)> {
    if !data.starts_with(b"V2\xff") {
        return Err(InputError::Structure);
    }
    let delimiters: [usize; 18] = data
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == 255).then_some(index))
        .take(18)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| InputError::Structure)?;
    let field = |index: usize| &data[delimiters[index - 1] + 1..delimiters[index]];
    for index in 1..18 {
        std::str::from_utf8(field(index)).map_err(|_| InputError::Structure)?;
    }
    let reference = field(2);
    if !matches!(field(1), b"0" | b"1" | b"2" | b"3")
        || reference.len() != 21
        || !reference.iter().all(u8::is_ascii_digit)
        || field(4).len() != 10
        || field(4).iter().enumerate().any(|(index, byte)| {
            if index == 2 || index == 5 {
                *byte != b'-'
            } else {
                !byte.is_ascii_digit()
            }
        })
        || !matches!(field(5), b"M" | b"F")
        || field(11).len() != 6
        || !field(11).iter().all(u8::is_ascii_digit)
        || field(13).len() > 31
        || data.len() <= delimiters[17] + 1
        || (data.len() + 9).div_ceil(64) * 64 - delimiters[17] - 1 > 1023
    {
        return Err(InputError::Structure);
    }
    let timestamp = NaiveDateTime::parse_from_str(
        std::str::from_utf8(&reference[4..18]).map_err(|_| InputError::Structure)?,
        "%Y%m%d%H%M%S",
    )
    .map_err(|_| InputError::Structure)?;
    let dob = NaiveDate::parse_from_str(
        std::str::from_utf8(field(4)).map_err(|_| InputError::Structure)?,
        "%d-%m-%Y",
    )
    .map_err(|_| InputError::Structure)?;
    if !(2000..2032).contains(&timestamp.year())
        || timestamp.and_utc().timestamp_subsec_nanos() != 0
        || !(1900..=timestamp.year()).contains(&dob.year())
        || dob > timestamp.date()
    {
        return Err(InputError::Structure);
    }
    let unix = timestamp.and_utc().timestamp() - 19800;
    Ok((
        delimiters,
        unix.try_into().map_err(|_| InputError::Structure)?,
    ))
}

fn limbs(bytes: &[u8]) -> Vec<String> {
    (0..17)
        .map(|limb| {
            let mut value = 0_u128;
            for bit in 0..121 {
                let position = limb * 121 + bit;
                if position / 8 < bytes.len() {
                    value |=
                        u128::from((bytes[bytes.len() - 1 - position / 8] >> (position % 8)) & 1)
                            << bit;
                }
            }
            value.to_string()
        })
        .collect()
}

pub fn prepare(
    decimal_qr: &[u8],
    modulus: &[u8; 256],
    nullifier_seed: &str,
    signal_hash: &str,
) -> Result<PreparedInput> {
    // This prepares circuit inputs, not a trusted receipt. The caller must select
    // the modulus from reviewed policy, check freshness, and verify the bound proof.
    if nullifier_seed == "0"
        || crate::groth16::scalar(nullifier_seed).is_err()
        || crate::groth16::scalar(signal_hash).is_err()
    {
        return Err(InputError::Binding);
    }
    if modulus[0] < 128 || modulus[255] & 1 == 0 {
        return Err(InputError::Signature);
    }
    let compressed = decimal_bytes(decimal_qr)?;
    let decoded = decompress(&compressed)?;
    let (data, signature) = decoded.split_at(decoded.len() - 256);
    RsaPublicKeyComponents {
        n: modulus.as_slice(),
        e: &[1, 0, 1],
    }
    .verify(&RSA_PKCS1_2048_8192_SHA256, data, signature)
    .map_err(|_| InputError::Signature)?;
    let (delimiters, exact_timestamp) = structure(data)?;
    let padded_length = (data.len() + 9).div_ceil(64) * 64;
    let mut padded = Zeroizing::new(vec![0_u8; PADDED_SIZE]);
    padded[..data.len()].copy_from_slice(data);
    padded[data.len()] = 128;
    padded[padded_length - 8..padded_length]
        .copy_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    Ok(PreparedInput {
        qr_data_padded: padded.iter().map(u8::to_string).collect(),
        qr_data_padded_length: padded_length.to_string(),
        delimiter_indices: delimiters.iter().map(usize::to_string).collect(),
        signature: limbs(signature),
        pub_key: limbs(modulus),
        nullifier_seed: nullifier_seed.into(),
        signal_hash: signal_hash.into(),
        reveal_age_above18: "0".into(),
        reveal_gender: "0".into(),
        reveal_pin_code: "0".into(),
        reveal_state: "0".into(),
        exact_timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, write::ZlibEncoder, Compression};
    use num_bigint::BigUint;
    use std::io::Write;

    fn fixture() -> (serde_json::Value, Vec<u8>, [u8; 256]) {
        let input: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/personhood-prover/synthetic-input.json"
        ))
        .unwrap();
        let padded: Vec<u8> = input["qrDataPadded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().parse().unwrap())
            .collect();
        let end: usize = input["qrDataPaddedLength"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let size = u64::from_be_bytes(padded[end - 8..end].try_into().unwrap()) / 8;
        let join = |name: &str| {
            let number = input[name].as_array().unwrap().iter().enumerate().fold(
                BigUint::from(0_u8),
                |value, (index, limb)| {
                    value + (limb.as_str().unwrap().parse::<BigUint>().unwrap() << (121 * index))
                },
            );
            let bytes = number.to_bytes_be();
            let mut padded = [0_u8; 256];
            padded[256 - bytes.len()..].copy_from_slice(&bytes);
            padded
        };
        let mut payload = padded[..size as usize].to_vec();
        payload.extend_from_slice(&join("signature"));
        let modulus = join("pubKey");
        (input, payload, modulus)
    }

    fn compressed(payload: &[u8], gzip: bool) -> Vec<u8> {
        if gzip {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(payload).unwrap();
            encoder.finish().unwrap()
        } else {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(payload).unwrap();
            encoder.finish().unwrap()
        }
    }

    fn decimal(payload: &[u8]) -> Vec<u8> {
        BigUint::from_bytes_be(payload).to_string().into_bytes()
    }

    #[test]
    fn signed_fixture_matches_pinned_sdk_witness_for_both_compression_formats() {
        let (expected, payload, modulus) = fixture();
        for gzip in [true, false] {
            let prepared = prepare(
                &decimal(&compressed(&payload, gzip)),
                &modulus,
                expected["nullifierSeed"].as_str().unwrap(),
                expected["signalHash"].as_str().unwrap(),
            )
            .unwrap();
            assert_eq!(serde_json::to_value(&prepared).unwrap(), expected);
            assert_eq!(prepared.exact_timestamp(), 1713557752);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&prepared.to_json().unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn decoding_rejects_aliases_trailing_members_truncation_and_bombs() {
        for input in [b"".as_slice(), b"0", b"01", b"-1", b"1\n", b"0x12"] {
            assert!(decimal_bytes(input).is_err());
        }
        assert!(decimal_bytes(&vec![b'9'; 4934]).is_err());
        for gzip in [true, false] {
            let encoded = compressed(&vec![0; MAX_DECODED + 1], gzip);
            assert!(decompress(&encoded).is_err());
            let valid = compressed(&vec![0; 300], gzip);
            assert_eq!(decompress(&valid).unwrap().len(), 300);
            for missing in 1..=8 {
                assert!(decompress(&valid[..valid.len() - missing]).is_err());
            }
            let mut corrupt = valid.clone();
            let last = corrupt.len() - 1;
            corrupt[last] ^= 1;
            assert!(decompress(&corrupt).is_err());
            let mut trailing = valid.clone();
            trailing.push(0);
            assert!(decompress(&trailing).is_err());
            assert!(decompress(&[valid.clone(), valid].concat()).is_err());
        }
    }

    #[test]
    fn decimal_conversion_preserves_bytes_and_enforces_capacity() {
        for length in [1, 2, 255, 256, 1783, MAX_COMPRESSED] {
            let bytes: Vec<u8> = (0..length).map(|index| (index % 255 + 1) as u8).collect();
            assert_eq!(decimal_bytes(&decimal(&bytes)).unwrap().as_slice(), bytes);
        }
        assert!(decimal_bytes(&decimal(&vec![255; MAX_COMPRESSED + 1])).is_err());
    }

    #[test]
    fn signature_and_context_changes_fail_before_witness_generation() {
        let (_, mut payload, modulus) = fixture();
        let valid = decimal(&compressed(&payload, false));
        for binding in ["01", "-1", "invalid"] {
            assert!(matches!(
                prepare(&valid, &modulus, binding, "1"),
                Err(InputError::Binding)
            ));
            assert!(matches!(
                prepare(&valid, &modulus, "1", binding),
                Err(InputError::Binding)
            ));
        }
        payload[30] ^= 1;
        assert!(matches!(
            prepare(&decimal(&compressed(&payload, false)), &modulus, "1", "1"),
            Err(InputError::Signature)
        ));
        let mut wrong_key = modulus;
        wrong_key[100] ^= 1;
        assert!(matches!(
            prepare(&valid, &wrong_key, "1", "1"),
            Err(InputError::Signature)
        ));
    }

    #[test]
    fn unsupported_structure_and_invalid_dates_are_rejected() {
        let (_, payload, _) = fixture();
        let data = &payload[..payload.len() - 256];
        let (delimiters, _) = structure(data).unwrap();
        for (index, byte) in [(1, b'1'), (3, b'4'), (13, b'9'), (delimiters[4] + 1, b'X')] {
            let mut changed = data.to_vec();
            changed[index] = byte;
            assert!(structure(&changed).is_err());
        }
        assert!(structure(&data[..delimiters[17] + 1]).is_err());
        let mut leap_second = data.to_vec();
        leap_second[21..23].copy_from_slice(b"60");
        assert!(structure(&leap_second).is_err());
    }
}
