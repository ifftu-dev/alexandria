use alexandria_personhood::{
    issue_challenge, local_verifier_key, signal_hash, Policy, FIXTURE_TIMESTAMP,
};
use ed25519_dalek::SigningKey;
fn main() {
    let account = SigningKey::from_bytes(&[1; 32]);
    let verifier = local_verifier_key(&account).unwrap();
    let policy = Policy::synthetic("test-network", &verifier.verifying_key());
    let challenge = issue_challenge(
        &policy,
        &account.verifying_key(),
        &verifier,
        [2; 32],
        [3; 32],
        FIXTURE_TIMESTAMP + 1,
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"signal_hash":signal_hash(&challenge.body).unwrap(),"policy":policy,"challenge":challenge})).unwrap());
}
