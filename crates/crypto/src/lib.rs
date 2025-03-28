// Copyright (c), Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::dem::Hmac256Ctr;
use crate::ibe::{decrypt_deterministic, encrypt_batched_deterministic};
use crate::tss::SecretSharing;
use dem::Aes256Gcm;
use fastcrypto::error::FastCryptoError::{GeneralError, InvalidInput};
use fastcrypto::error::FastCryptoResult;
use fastcrypto::groups::Scalar;
use fastcrypto::hmac::{hmac_sha3_256, HmacKey};
use itertools::Itertools;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::HashMap;
pub use sui_types::base_types::ObjectID;
use sui_types::crypto::ToFromBytes;
use tss::{combine, split};
use utils::generate_random_bytes;

pub mod dem;
pub mod elgamal;
pub mod gf256;
pub mod ibe;
mod polynomial;
pub mod tss;
mod utils;

/// The domain separation tag for the hash-to-group function.
pub const DST: &[u8] = b"SUI-SEAL-IBE-BLS12381-00";

/// The domain separation tag for the hash-to-group function.
pub const DST_POP: &[u8] = b"SUI-SEAL-IBE-BLS12381-POP-00";

pub const KEY_SIZE: usize = 32;

/// This represents an encrypted object.
/// Must be aligned with TypeScript type.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedObject {
    pub version: u8,
    pub package_id: ObjectID,
    pub id: Vec<u8>,
    // The address for a key server + the index of the share held by this server
    pub services: Vec<(ObjectID, u8)>,
    pub threshold: u8,
    pub encrypted_shares: IBEEncryptions,
    pub ciphertext: Ciphertext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Ciphertext {
    Aes256Gcm {
        blob: Vec<u8>,
        aad: Option<Vec<u8>>,
    },
    Hmac256Ctr {
        blob: Vec<u8>,
        aad: Option<Vec<u8>>,
        mac: [u8; KEY_SIZE],
    },
    Plain,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IBEEncryptions {
    BonehFranklinBLS12381 {
        nonce: ibe::Nonce,
        encrypted_shares: Vec<[u8; KEY_SIZE]>,
        encrypted_randomness: [u8; KEY_SIZE],
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IBEPublicKeys {
    BonehFranklinBLS12381(Vec<ibe::PublicKey>),
}

pub enum IBEUserSecretKeys {
    BonehFranklinBLS12381(HashMap<ObjectID, ibe::UserSecretKey>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EncryptionInput {
    Aes256Gcm { data: Vec<u8>, aad: Option<Vec<u8>> },
    Hmac256Ctr { data: Vec<u8>, aad: Option<Vec<u8>> },
    Plain,
}

/// Encrypt the given plaintext. This is done as follows:
///  - Generate a random AES key and encrypt the message under this key,
///  - Secret share the key with one share per key-server using the protocol defined in the tss module,
///  - For each key-server, encrypt the share using IBE,
///  - Return the ciphertext, encrypted shares, and the common key encryption nonce.
///
/// @param package_id The package id.
/// @param id The id.
/// @param key_servers The seal key services to use for the encryption.
/// @param public_keys The public keys of the key servers.
/// @param threshold The threshold for the TSS.
/// @param encryption_input The encryption input.
/// @return The encrypted object and the derived symmetric key used for the encryption.
pub fn seal_encrypt(
    package_id: ObjectID,
    id: Vec<u8>,
    key_servers: Vec<ObjectID>,
    public_keys: &IBEPublicKeys,
    threshold: u8,
    encryption_input: EncryptionInput,
) -> FastCryptoResult<(EncryptedObject, [u8; KEY_SIZE])> {
    let number_of_shares = key_servers.len() as u8;
    if threshold > number_of_shares || threshold == 0 {
        return Err(InvalidInput);
    }

    let mut rng = thread_rng();
    let full_id = create_full_id(&package_id, &id);

    // Generate a random base key
    let base_key = generate_random_bytes(&mut rng);

    // Derive the key used by the DEM
    let dem_key = derive_key(KeyPurpose::DEM, &base_key);
    let ciphertext = match encryption_input {
        EncryptionInput::Aes256Gcm { data, aad } => Ciphertext::Aes256Gcm {
            blob: Aes256Gcm::encrypt(&data, aad.as_ref().unwrap_or(&vec![]), &dem_key),
            aad,
        },
        EncryptionInput::Hmac256Ctr { data, aad } => {
            let (blob, mac) = Hmac256Ctr::encrypt(&data, aad.as_ref().unwrap_or(&vec![]), &dem_key);
            Ciphertext::Hmac256Ctr { blob, mac, aad }
        }
        EncryptionInput::Plain => Ciphertext::Plain,
    };

    // Secret share the derived key
    let SecretSharing {
        indices, shares, ..
    } = split(&mut rng, base_key, threshold, number_of_shares)?;

    let services = key_servers.into_iter().zip(indices).collect::<Vec<_>>();

    let encrypted_shares = match public_keys {
        IBEPublicKeys::BonehFranklinBLS12381(public_keys) => {
            if public_keys.len() != number_of_shares as usize {
                return Err(InvalidInput);
            }
            let randomness = ibe::Randomness::rand(&mut rng);

            // Encrypt the shares using the IBE keys.
            // Use the share index as the `index` parameter for the IBE decryption, allowing to encrypt shares for the same identity to the same public key.
            let (nonce, encrypted_shares) = encrypt_batched_deterministic(
                &randomness,
                &shares,
                public_keys,
                &full_id,
                &services,
            )?;

            let encrypted_randomness = ibe::encrypt_randomness(
                &randomness,
                &derive_key(KeyPurpose::EncryptedRandomness, &base_key),
            );
            IBEEncryptions::BonehFranklinBLS12381 {
                nonce,
                encrypted_shares,
                encrypted_randomness,
            }
        }
    };

    Ok((
        EncryptedObject {
            version: 0,
            package_id,
            id,
            services,
            threshold,
            encrypted_shares,
            ciphertext,
        },
        dem_key,
    ))
}

/// Decrypt the given ciphertext as follows:
///  - Decapsulate the IBE keys for the given nonce and user secret keys,
///  - Decrypt the shares using the deltas,
///  - Reconstruct the AES key from the shares,
///  - Decrypt the ciphertext using the AES key.
///
/// @param encrypted_object The encrypted object. See `seal_encrypt`.
/// @param user_secret_keys The user secret keys. It's assumed that these are validated. Otherwise, the decryption will fail or, eg. in the case of using `Plain` mode, the derived key will be wrong.
/// @param public_keys The public keys of the key servers. If provided, all shares will be decrypted and checked for consistency.
/// @return The decrypted plaintext or, if `Plain` mode was used, the derived key.
pub fn seal_decrypt(
    encrypted_object: &EncryptedObject,
    user_secret_keys: &IBEUserSecretKeys,
    public_keys: Option<&IBEPublicKeys>,
) -> FastCryptoResult<Vec<u8>> {
    let EncryptedObject {
        version,
        package_id,
        id,
        encrypted_shares,
        services,
        threshold,
        ciphertext,
        ..
    } = encrypted_object;

    if *version != 0 {
        return Err(InvalidInput);
    }

    let full_id = create_full_id(package_id, id);

    // Decap IBE keys and decrypt shares
    let shares = match (&encrypted_shares, user_secret_keys) {
        (
            IBEEncryptions::BonehFranklinBLS12381 {
                nonce,
                encrypted_shares,
                ..
            },
            IBEUserSecretKeys::BonehFranklinBLS12381(user_secret_keys),
        ) => {
            // Check that the encrypted object is valid,
            // e.g., that there is an encrypted share of the key per service
            if encrypted_shares.len() != services.len() {
                return Err(InvalidInput);
            }

            // The indices of the services for which we have a secret key
            let service_indices: Vec<usize> = services
                .iter()
                .enumerate()
                .filter(|(_, (id, _))| user_secret_keys.contains_key(id))
                .map(|(i, _)| i)
                .collect();
            if service_indices.len() < *threshold as usize {
                return Err(InvalidInput);
            }

            service_indices
                .into_iter()
                .map(|i| {
                    let index = services[i].1;
                    (index, ibe::decrypt(
                        nonce,
                        &encrypted_shares[i],
                        user_secret_keys
                            .get(&services[i].0)
                            .expect("This shouldn't happen: It's checked above that this secret key is available"),
                        &full_id,
                        &services[i],
                    ))
                })
                .collect_vec()
        }
    };

    // Create the base key from the shares and decrypt
    let base_key = combine(&shares)?;

    // If desired, after we have the encryption key, we can decrypt all shares and check for consistency
    if let Some(public_keys) = public_keys {
        let all_shares = encrypted_shares.decrypt_all_shares(
            &full_id,
            services,
            public_keys,
            &derive_key(KeyPurpose::EncryptedRandomness, &base_key),
        )?;

        let reconstructed_base_key = combine(
            &services
                .iter()
                .zip(all_shares)
                .map(|((_, i), share)| (*i, share))
                .collect_vec(),
        )?;
        if reconstructed_base_key != base_key {
            return Err(GeneralError("Invalid secret sharing given".to_string()));
        }
        // TODO: The above is just a sanity check. We need to check that the interpolated polynomial from the given shares has the remaining shares as points. The current check just checks that the constant term is the same. But that doesn't rule out that another subset of the shares would've given a different secret.
    }

    let dem_key = derive_key(KeyPurpose::DEM, &base_key);
    match ciphertext {
        Ciphertext::Aes256Gcm { blob, aad } => {
            Aes256Gcm::decrypt(blob, aad.as_ref().map_or(&[], |v| v), &dem_key)
        }
        Ciphertext::Hmac256Ctr { blob, aad, mac } => {
            Hmac256Ctr::decrypt(blob, mac, aad.as_ref().map_or(&[], |v| v), &dem_key)
        }
        Ciphertext::Plain => Ok(dem_key.to_vec()),
    }
}

/// Create a full id from the [DST], a package id and an inner id. The result has the following format:
/// [len(DST)][DST][package_id][id]
pub fn create_full_id(package_id: &[u8; 32], id: &[u8]) -> Vec<u8> {
    assert!(DST.len() < 256);
    let mut full_id = vec![DST.len() as u8];
    full_id.extend_from_slice(DST);
    full_id.extend_from_slice(package_id);
    full_id.extend_from_slice(id);
    full_id
}

/// An enum representing the different purposes of the derived key.
pub enum KeyPurpose {
    /// The key used to encrypt the encryption randomness.
    EncryptedRandomness,
    /// The key used by the DEM.
    DEM,
}

/// Derive a key for a specific purpose from the base key.
fn derive_key(purpose: KeyPurpose, derived_key: &[u8; KEY_SIZE]) -> [u8; KEY_SIZE] {
    let hmac_key = HmacKey::from_bytes(derived_key).expect("Fixed length");
    match purpose {
        KeyPurpose::EncryptedRandomness => hmac_sha3_256(&hmac_key, &[0]).digest,
        KeyPurpose::DEM => hmac_sha3_256(&hmac_key, &[1]).digest,
    }
}

impl IBEEncryptions {
    /// Given the derived key, decrypt all shares
    fn decrypt_all_shares(
        &self,
        id: &[u8],
        services: &[(ObjectID, u8)],
        public_keys: &IBEPublicKeys,
        key: &[u8; KEY_SIZE],
    ) -> FastCryptoResult<Vec<[u8; KEY_SIZE]>> {
        match self {
            IBEEncryptions::BonehFranklinBLS12381 {
                encrypted_randomness,
                encrypted_shares,
                nonce,
            } => {
                // Decrypt encrypted nonce,
                let nonce = ibe::decrypt_and_verify_nonce(encrypted_randomness, key, nonce)?;

                // Decrypt all shares
                match public_keys {
                    IBEPublicKeys::BonehFranklinBLS12381(public_keys) => public_keys
                        .iter()
                        .zip(encrypted_shares)
                        .zip(services)
                        .map(|((pk, s), service)| decrypt_deterministic(&nonce, s, pk, id, service))
                        .collect::<FastCryptoResult<Vec<_>>>(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastcrypto::{
        encoding::{Base64, Encoding},
        groups::{
            bls12381::{G1Element, Scalar},
            HashToGroupElement,
        },
        serde_helpers::ToFromByteArray,
    };
    use std::str::FromStr;

    #[test]
    fn test_hash_with_prefix_regression() {
        let hash = G1Element::hash_to_group_element(&create_full_id(
            &ObjectID::from_bytes([0u8; 32]).unwrap(),
            &[1, 2, 3, 4],
        ));
        assert_eq!(hex::encode(hash.to_byte_array()), "b32685b6ffd1f373faf3abb10c05772e033f75da8af729c3611d81aea845670db48ceadd0132d3a667dbbaa36acefac7");
    }

    #[test]
    fn test_encryption_round_trip_aes() {
        let data = b"Hello, World!";
        let package_id = ObjectID::random();
        let id = vec![1, 2, 3, 4];

        let full_id = create_full_id(&package_id, &id);

        let mut rng = rand::thread_rng();
        let keypairs = (0..3)
            .map(|_| ibe::generate_key_pair(&mut rng))
            .collect_vec();

        let services = keypairs.iter().map(|_| ObjectID::random()).collect_vec();

        let threshold = 2;
        let public_keys =
            IBEPublicKeys::BonehFranklinBLS12381(keypairs.iter().map(|(_, pk)| *pk).collect_vec());

        let encrypted = seal_encrypt(
            package_id,
            id,
            services.clone(),
            &public_keys,
            threshold,
            EncryptionInput::Aes256Gcm {
                data: data.to_vec(),
                aad: Some(b"something".to_vec()),
            },
        )
        .unwrap()
        .0;

        let user_secret_keys = IBEUserSecretKeys::BonehFranklinBLS12381(
            services
                .into_iter()
                .zip(keypairs)
                .map(|(s, kp)| (s, ibe::extract(&kp.0, &full_id)))
                .collect(),
        );
        let decrypted = seal_decrypt(&encrypted, &user_secret_keys, Some(&public_keys)).unwrap();

        assert_eq!(data, decrypted.as_slice());

        // Check that decryption fails with different aad
        let mut modified_encrypted = encrypted.clone();
        match modified_encrypted.ciphertext {
            Ciphertext::Aes256Gcm { ref mut aad, .. } => {
                match aad {
                    None => panic!(),
                    Some(ref mut aad) => aad.push(0),
                }
                assert!(
                    seal_decrypt(&modified_encrypted, &user_secret_keys, Some(&public_keys))
                        .is_err()
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_encryption_round_trip_hmac() {
        let data = b"Hello, World!";
        let package_id = ObjectID::random();
        let id = vec![1, 2, 3, 4];

        let full_id = create_full_id(&package_id, &id);

        let mut rng = rand::thread_rng();
        let keypairs = (0..3)
            .map(|_| ibe::generate_key_pair(&mut rng))
            .collect_vec();

        let services = keypairs.iter().map(|_| ObjectID::random()).collect_vec();

        let threshold = 2;
        let public_keys =
            IBEPublicKeys::BonehFranklinBLS12381(keypairs.iter().map(|(_, pk)| *pk).collect_vec());

        let encrypted = seal_encrypt(
            package_id,
            id,
            services.clone(),
            &public_keys,
            threshold,
            EncryptionInput::Hmac256Ctr {
                data: data.to_vec(),
                aad: Some(b"something".to_vec()),
            },
        )
        .unwrap()
        .0;

        let user_secret_keys = IBEUserSecretKeys::BonehFranklinBLS12381(
            services
                .into_iter()
                .zip(keypairs)
                .map(|(s, kp)| (s, ibe::extract(&kp.0, &full_id)))
                .collect(),
        );
        let decrypted = seal_decrypt(&encrypted, &user_secret_keys, Some(&public_keys)).unwrap();

        assert_eq!(data, decrypted.as_slice());

        // Check that decryption fails with different aad
        let mut modified_encrypted = encrypted.clone();
        match modified_encrypted.ciphertext {
            Ciphertext::Hmac256Ctr { ref mut aad, .. } => {
                match aad {
                    None => panic!(),
                    Some(ref mut aad) => aad.push(0),
                }
                assert!(
                    seal_decrypt(&modified_encrypted, &user_secret_keys, Some(&public_keys))
                        .is_err()
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_plain_round_trip() {
        let package_id = ObjectID::random();
        let id = vec![1, 2, 3, 4];
        let full_id = create_full_id(&package_id, &id);

        let mut rng = rand::thread_rng();
        let keypairs = (0..3)
            .map(|_| ibe::generate_key_pair(&mut rng))
            .collect_vec();

        let services = keypairs.iter().map(|_| ObjectID::random()).collect_vec();

        let threshold = 2;
        let public_keys =
            IBEPublicKeys::BonehFranklinBLS12381(keypairs.iter().map(|(_, pk)| *pk).collect_vec());

        let (encrypted, key) = seal_encrypt(
            package_id,
            id,
            services.clone(),
            &public_keys,
            threshold,
            EncryptionInput::Plain,
        )
        .unwrap();

        let user_secret_keys = services
            .into_iter()
            .zip(keypairs)
            .map(|(s, kp)| (s, ibe::extract(&kp.0, &full_id)))
            .collect();

        assert_eq!(
            key.to_vec(),
            seal_decrypt(
                &encrypted,
                &IBEUserSecretKeys::BonehFranklinBLS12381(user_secret_keys),
                Some(&public_keys),
            )
            .unwrap()
        );
    }

    #[test]
    fn typescript_test_vector() {
        let package_id = [0u8; 32];
        let inner_id = [1, 2, 3, 4];

        let master_keys = [
            "LabXNEuHRj8q0Zs2K/zZ1uKGLxU49yNWMkZ7nH6qs5o=",
            "B0dcGoRo16zDecMjRIXmhvUz8upetnN7ro28sdu6mjw=",
            "N9d6H24erQHDFTRJrF+PyNt1MEPHXwxwS/p35BkgHh0=",
        ]
        .iter()
        .map(|key| {
            Scalar::from_byte_array(&Base64::decode(key).unwrap().try_into().unwrap()).unwrap()
        })
        .collect::<Vec<_>>();
        let public_keys = master_keys
            .iter()
            .map(ibe::public_key_from_master_key)
            .collect_vec();

        let encryption = Base64::decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAECAwQDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFtAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIpAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAOUAgCNunXOscCObhBWWvYBnWtkOL7L8bQT37A9zf5mCsde1bFZLfl4pkvOQtfTmO15Da8OaDPwuYT38Yh92Qc5tyHOnNGTFzzX4dlfsAMbLLsLQSEnldHdekxYIfovbvErtx4Dj98TCnfpOxvrIHQ9ofLrV4HEE8gBBuVzMijpQgUMJaeTjRt7wJhugjszCT+csp6lBBM+Vy4Z0Hwtl22uL7vCwN02rFUQdqatcHQjG8Ff6zMU1uFhw05oQXzQOXUSa6lCnuR50I9qSsYoysARKGL8cdnto6lF9gb8EsusC0bDU9gAJzTOz9G+4cvrUyVBXb4T6MW9QX/xnKxF+WOAM5T2BasXBqxJcBnvOAEEAQIDBA==").unwrap();
        let encryption: EncryptedObject = bcs::from_bytes(&encryption).unwrap();

        let object_ids = [
            "0x0000000000000000000000000000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000000000000000000000000000003",
        ]
        .iter()
        .map(|id| ObjectID::from_str(id).unwrap())
        .collect::<Vec<_>>();

        let full_id = create_full_id(&package_id, &inner_id);
        let user_secret_keys = object_ids
            .into_iter()
            .zip(master_keys)
            .map(|(s, k)| (s, ibe::extract(&k, &full_id)))
            .collect();

        let decrypted = seal_decrypt(
            &encryption,
            &IBEUserSecretKeys::BonehFranklinBLS12381(user_secret_keys),
            Some(&IBEPublicKeys::BonehFranklinBLS12381(public_keys)),
        )
        .unwrap();

        assert_eq!(decrypted, b"My super secret message");
    }
}
