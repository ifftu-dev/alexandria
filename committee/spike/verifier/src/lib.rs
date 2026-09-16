#![no_std]

use cometbft::block::signed_header::SignedHeader;
use cometbft_light_client_verifier::{ProdVerifier, Verifier};

pub fn verifier_type_is_available() -> usize {
    core::mem::size_of::<ProdVerifier>() + core::mem::size_of::<SignedHeader>()
}

pub fn verifier_trait_is_available(verifier: &impl Verifier) -> usize {
    core::mem::size_of_val(verifier)
}
