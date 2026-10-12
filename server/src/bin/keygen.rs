use base64::{Engine as _, engine::general_purpose::STANDARD};

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

fn main() {
    let mut rng = OsRng;

    let signing_key = SigningKey::generate(&mut rng);

    let verifying_key = signing_key.verifying_key();

    println!(
        "PATCHFORGE_PRIVATE_KEY={}",
        STANDARD.encode(signing_key.to_bytes())
    );

    println!(
        "PATCHFORGE_PUBLIC_KEY={}",
        STANDARD.encode(verifying_key.to_bytes())
    );
}
