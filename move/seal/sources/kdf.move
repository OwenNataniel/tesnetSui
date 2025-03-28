module seal::kdf;

use sui::{bls12381::{G1, G2, GT}, group_ops::Element, hmac::hmac_sha3_256};

public(package) fun kdf(
    input: &Element<GT>,
    nonce: &Element<G2>,
    gid: &Element<G1>,
    object_id: address,
    info: &vector<u8>,
): vector<u8> {
    let mut bytes = *input.bytes();
    bytes.append(*nonce.bytes());
    bytes.append(*gid.bytes());
    bytes.append(object_id.to_bytes());

    hkdf_sha3_256(
        &bytes,
        &x"0000000000000000000000000000000000000000000000000000000000000000",
        info,
    )
}

// Fixed to 32 bytes. Must give non-empty salt.
fun hkdf_sha3_256(ikm: &vector<u8>, salt: &vector<u8>, info: &vector<u8>): vector<u8> {
    assert!(!salt.is_empty());
    let mut t = *info;
    t.push_back(1);
    hmac_sha3_256(&hmac_sha3_256(salt, ikm), &t)
}

#[test]
fun test_kdf() {
    use sui::bls12381::{scalar_from_u64, g2_generator, gt_generator, g2_mul, hash_to_g1, gt_mul};
    let r = scalar_from_u64(12345u64);
    let x = gt_mul(&r, &gt_generator());
    let nonce = g2_mul(&r, &g2_generator());
    let gid = hash_to_g1(&vector[0]);
    let derived_key = kdf(&x, &nonce, &gid, @0x0, &vector[42]);
    let expected = x"71a8b3d86252de91f4aab16b641fc5f11fc7999e3d2b5c4814985a30e99ab9f9";
    assert!(derived_key == expected);
}
