//! Basic example showing how to use Seal for simple cryptographic operations

use seal::crypto::PublicKey;
use seal::hash::{Digest, Sha256};
use seal::encoding::Base64;

fn main() {
    println!("Seal Basic Example");
    println!("==================");
    
    // Hashing example
    let data = "Hello, Seal!";
    let hash = Sha256::digest(data.as_bytes());
    println!("SHA-256 hash of '{}': {}", data, Base64::encode(&hash));
    
    // Key generation example
    let key = PublicKey::random();
    println!("Generated public key: {}", Base64::encode(&key.to_bytes()));
    
    println!("Example completed successfully!");
}
