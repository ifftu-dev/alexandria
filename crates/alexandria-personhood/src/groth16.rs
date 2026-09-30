use ark_bn254::{Bn254, Fq, Fq2, Fr, G1Affine, G2Affine};
use ark_ff::PrimeField;
use ark_groth16::{prepare_verifying_key, Groth16, Proof, VerifyingKey};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{decode, Error, Result};

pub use alexandria_verify::personhood::VERIFICATION_KEY_SHA256;
const KEY: &[u8] = include_bytes!("../assets/vkey.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Groth16Proof {
    pub pi_a: [String; 3],
    pub pi_b: [[String; 2]; 3],
    pub pi_c: [String; 3],
    pub protocol: String,
    #[serde(default = "curve")]
    pub curve: String,
}

#[derive(Deserialize)]
struct JsonKey {
    vk_alpha_1: [String; 3],
    vk_beta_2: [[String; 2]; 3],
    vk_gamma_2: [[String; 2]; 3],
    vk_delta_2: [[String; 2]; 3],
    #[serde(rename = "IC")]
    ic: Vec<[String; 3]>,
}

fn field<F: PrimeField>(value: &str) -> Result<F> {
    if value.is_empty()
        || value.len() > 78
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(Error::Encoding);
    }
    let n: BigUint = value.parse().map_err(|_| Error::Encoding)?;
    if n >= F::MODULUS.into() {
        return Err(Error::Encoding);
    }
    Ok(F::from_be_bytes_mod_order(&n.to_bytes_be()))
}

pub fn scalar(value: &str) -> Result<()> {
    field::<Fr>(value).map(|_| ())
}

fn g1(value: &[String; 3]) -> Result<G1Affine> {
    if value[2] != "1" {
        return Err(Error::Point);
    }
    let point = G1Affine::new_unchecked(field::<Fq>(&value[0])?, field::<Fq>(&value[1])?);
    if !point.is_on_curve() || !point.is_in_correct_subgroup_assuming_on_curve() {
        return Err(Error::Point);
    }
    Ok(point)
}

fn g2(value: &[[String; 2]; 3]) -> Result<G2Affine> {
    if value[2] != ["1", "0"] {
        return Err(Error::Point);
    }
    let point = G2Affine::new_unchecked(
        Fq2::new(field::<Fq>(&value[0][0])?, field::<Fq>(&value[0][1])?),
        Fq2::new(field::<Fq>(&value[1][0])?, field::<Fq>(&value[1][1])?),
    );
    if !point.is_on_curve() || !point.is_in_correct_subgroup_assuming_on_curve() {
        return Err(Error::Point);
    }
    Ok(point)
}

pub fn verify(proof: &Groth16Proof, signals: &[String; 9]) -> Result<()> {
    if proof.protocol != "groth16" || proof.curve != "bn128" {
        return Err(Error::Proof);
    }
    let input = signals
        .iter()
        .map(|s| field::<Fr>(s))
        .collect::<Result<Vec<_>>>()?;
    let proof = Proof::<Bn254> {
        a: g1(&proof.pi_a)?,
        b: g2(&proof.pi_b)?,
        c: g1(&proof.pi_c)?,
    };
    if hex::encode(Sha256::digest(KEY)) != VERIFICATION_KEY_SHA256 {
        return Err(Error::Policy);
    }
    let json: JsonKey = decode(KEY)?;
    if json.ic.len() != 10 {
        return Err(Error::Policy);
    }
    let key = VerifyingKey::<Bn254> {
        alpha_g1: g1(&json.vk_alpha_1)?,
        beta_g2: g2(&json.vk_beta_2)?,
        gamma_g2: g2(&json.vk_gamma_2)?,
        delta_g2: g2(&json.vk_delta_2)?,
        gamma_abc_g1: json.ic.iter().map(g1).collect::<Result<Vec<_>>>()?,
    };
    if !Groth16::<Bn254>::verify_proof(&prepare_verifying_key(&key), &proof, &input)
        .map_err(|_| Error::Proof)?
    {
        return Err(Error::Proof);
    }
    Ok(())
}

fn curve() -> String {
    "bn128".into()
}
