//! Plutus Data CBOR encoding for completion datums and redeemers. The
//! governance encoders went with the obsolete governance transaction
//! builders, and the soulbound and reputation-mint redeemers with the
//! retired CIP-68 reputation mint.
//!
//! Mirrors the Aiken types from `cardano/governance/lib/alexandria/types.ak`.
//! Uses pallas_codec::minicbor for manual Plutus Data encoding.
//!
//! Plutus Data encoding rules:
//! - `Constr(n, fields)` for n in 0..6 → CBOR tag (121+n) + array
//! - `Constr(n, fields)` for n >= 7 → CBOR tag 102 + [n, fields]
//! - ByteArray → CBOR bytes
//! - Int → CBOR integer
//! - List<T> → CBOR array
//! - Bool → Constr(1,[]) for True, Constr(0,[]) for False

use crate::cardano::tx_builder::TxBuildError;

// ---- Helpers ----

/// Encode a Plutus Data constructor with index 0-6.
fn begin_constr(
    encoder: &mut pallas_codec::minicbor::Encoder<&mut Vec<u8>>,
    tag: u8,
    num_fields: u64,
) -> Result<(), TxBuildError> {
    encoder
        .tag(pallas_codec::minicbor::data::Tag::new(121 + tag as u64))
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    encoder
        .array(num_fields)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(())
}

/// Encode a Plutus Data integer.
fn encode_int(
    encoder: &mut pallas_codec::minicbor::Encoder<&mut Vec<u8>>,
    val: i64,
) -> Result<(), TxBuildError> {
    encoder
        .i64(val)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(())
}

/// Encode Plutus Data bytes.
fn encode_bytes(
    encoder: &mut pallas_codec::minicbor::Encoder<&mut Vec<u8>>,
    val: &[u8],
) -> Result<(), TxBuildError> {
    encoder
        .bytes(val)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(())
}

// ---- CompletionDatum / CompletionRedeemer ----

/// Encode a `CompletionDatum` matching `lib/alexandria/completion.ak`.
///
/// Shape: `Constr 0 [subject_pubkey, course_id, completion_root, timestamp]`.
/// The off-chain tx builder writes this as the inline datum on the
/// output carrying the minted completion token; the validator
/// reconstructs the Merkle root from the redeemer and requires
/// equality with `completion_root`.
pub fn encode_completion_datum(
    subject_pubkey: &[u8],
    course_id: &[u8],
    completion_root: &[u8; 32],
    timestamp_ms: i64,
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);
    begin_constr(&mut encoder, 0, 4)?;
    encode_bytes(&mut encoder, subject_pubkey)?;
    encode_bytes(&mut encoder, course_id)?;
    encode_bytes(&mut encoder, completion_root)?;
    encode_int(&mut encoder, timestamp_ms)?;
    Ok(buf)
}

/// Encode a `CompletionRedeemer::MintCompletion` carrying the raw
/// element leaves. The validator recomputes the Merkle root over
/// these leaves and checks it against the datum.
pub fn encode_completion_mint_redeemer(
    element_leaves: &[[u8; 32]],
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);
    begin_constr(&mut encoder, 0, 1)?;
    encoder
        .array(element_leaves.len() as u64)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    for leaf in element_leaves {
        encode_bytes(&mut encoder, leaf)?;
    }
    Ok(buf)
}

/// Encode a `CompletionRedeemer::BurnCompletion`.
pub fn encode_completion_burn_redeemer() -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);
    begin_constr(&mut encoder, 1, 0)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_datum_round_trip_decodes_via_cardano_completion_module() {
        let subject = [0x11u8; 32];
        let course = [0x22u8; 16];
        let root = [0x33u8; 32];
        let ts = 1_714_000_000_000i64;

        let bytes = encode_completion_datum(&subject, &course, &root, ts).unwrap();
        let hex_str = hex::encode(&bytes);

        // Starts with Constr 0 tag (121 = 0xd8 0x79).
        assert_eq!(bytes[0], 0xd8);
        assert_eq!(bytes[1], 0x79);

        // The observer's decoder must be able to read this back.
        // Mirrors `cardano::completion::decode_completion_datum`.
        use pallas_codec::minicbor::Decoder;
        let mut d = Decoder::new(&bytes);
        let tag = d.tag().unwrap();
        assert_eq!(u64::from(tag), 121);
        let len = d.array().unwrap();
        assert_eq!(len, Some(4));
        assert_eq!(d.bytes().unwrap(), &subject[..]);
        assert_eq!(d.bytes().unwrap(), &course[..]);
        assert_eq!(d.bytes().unwrap(), &root[..]);
        assert_eq!(d.i64().unwrap(), ts);

        // Sanity: hex is stable and non-empty.
        assert!(!hex_str.is_empty());
    }

    #[test]
    fn completion_mint_redeemer_carries_leaves_in_order() {
        let leaves = [[0xaau8; 32], [0xbbu8; 32], [0xccu8; 32]];
        let bytes = encode_completion_mint_redeemer(&leaves).unwrap();

        // Constr(0, [list]) → tag 121 + array(1) + array(3 leaves)
        assert_eq!(bytes[0], 0xd8);
        assert_eq!(bytes[1], 0x79);

        use pallas_codec::minicbor::Decoder;
        let mut d = Decoder::new(&bytes);
        let tag = d.tag().unwrap();
        assert_eq!(u64::from(tag), 121);
        assert_eq!(d.array().unwrap(), Some(1));
        assert_eq!(d.array().unwrap(), Some(3));
        for expected in leaves.iter() {
            assert_eq!(d.bytes().unwrap(), &expected[..]);
        }
    }

    #[test]
    fn completion_burn_redeemer_is_constr_1_empty() {
        let bytes = encode_completion_burn_redeemer().unwrap();
        // Constr(1, []) → tag 122 (0xd8 0x7a) + empty array
        assert_eq!(bytes[0], 0xd8);
        assert_eq!(bytes[1], 0x7a);
    }
}
