// Copyright (c), Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Voting pattern:
/// - Anyone can create a vote with a set of voters.
/// - The voters can submit their encrypted votes.
/// - After all the voters have submitted votes, anyone can retrieve the encryption keys and submit them. The votes are then decrypted on-chain.
///
/// This is an example of on-chain decryption. Other use cases of this include auctions, timelocked voting, etc.
module patterns::voting;

use seal::bf_hmac_encryption::{EncryptedObject, VerifiedDerivedKey, PublicKey, decrypt};

const EInvalidVote: u64 = 1;
const EVoteNotDone: u64 = 2;
const EAlreadyFinalized: u64 = 3;

/// This represents a vote.
public struct Vote has key {
    /// The id of a vote is the id of the object.
    id: UID,
    /// The eligble voters of the vote.
    voters: vector<address>,
    /// This holds the encrypted votes assuming the same order as the `voters` vector.
    votes: vector<Option<EncryptedObject>>,
    /// This will be set after the vote is finalised. The vote options are represented by a Option<u8> which is None if the vote was invalid.
    result: Option<vector<Option<u8>>>,
    /// The key servers that must be used for the encryption of the votes.
    key_servers: vector<ID>,
    /// The threshold for the vote.
    threshold: u8,
}

// The id of a vote is the id of the object.
public fun id(v: &Vote): vector<u8> {
    object::id(v).to_bytes()
}

#[test_only]
public fun destroy_for_testing(v: Vote) {
    let Vote { id, .. } = v;
    object::delete(id);
}

/// Create a vote.
/// The associated key-ids are [pkg id][vote id].
public fun create_vote(
    voters: vector<address>,
    key_servers: vector<ID>,
    threshold: u8,
    ctx: &mut TxContext,
): Vote {
    assert!(threshold <= key_servers.length() as u8);
    Vote {
        id: object::new(ctx),
        voters,
        key_servers,
        threshold,
        votes: vector::tabulate!(voters.length(), |_| option::none()),
        result: option::none(),
    }
}

/// Cast a vote.
/// The encrypted object should be an encryption of a single u8 and have the senders address as aad.
public fun cast_vote(vote: &mut Vote, encrypted_vote: EncryptedObject, ctx: &mut TxContext) {
    // The voter id must be put as aad to ensure that an encrypted vote cannot be copied and cast by another voter.
    assert!(encrypted_vote.aad().borrow() == ctx.sender().to_bytes(), EInvalidVote);

    // All encrypted vote must have been encrypted using the same key servers and the same threshold.
    // We could allow the order of the key servers to be different, but for the sake of simplicity, we also require the same order.
    assert!(encrypted_vote.services() == vote.key_servers.map_ref!(|id| id.to_address()));
    assert!(encrypted_vote.threshold() == vote.threshold);

    // This aborts if the sender is not a voter.
    let index = vote.voters.find_index!(|voter| voter == ctx.sender()).extract();
    vote.votes[index].fill(encrypted_vote);
}

entry fun seal_approve(id: vector<u8>, vote: &Vote) {
    assert!(id == object::id(vote).to_bytes(), EInvalidVote);
    assert!(vote.votes.all!(|vote| vote.is_some()), EVoteNotDone);
}

/// Finalize a vote.
/// Updates the `result` field of the vote to hold the votes of the corresponding voters.
/// Aborts if the vote has already been finalized.
/// Aborts if there are not enough keys or if they are not valid, e.g. if they were derived for a different purpose.
/// In case the keys are valid but a vote, is invalid, decrypt will just set the corresponding result to none.
public fun finalize_vote(
    vote: &mut Vote,
    keys: &vector<VerifiedDerivedKey>,
    public_keys: &vector<PublicKey>,
) {
    assert!(vote.result.is_none(), EAlreadyFinalized);

    // This aborts if there are not enough keys or if they are invalid, e.g. if they were derived for a different purpose.
    // However, in case the keys are valid but some of the encrypted objects, aka the votes, are invalid, decrypt will just return none for these votes.
    vote.result.fill(vote.votes.map_ref!(|vote| {
        let decrypted = decrypt(vote.borrow(), keys, public_keys);
        if (decrypted.is_some()) {
            let v = decrypted.borrow();
            // We expect the vote to be a single byte.
            if (v.length() == 1) {
                return option::some(v[0])
            }
        };
        option::none()
    }));
    // The encrypted votes can be deleted here if they are not needed anymore.
}

#[test]
fun test_vote() {
    use seal::bf_hmac_encryption::{verify_derived_keys, get_public_key};
    use seal::key_server::{register, destroy_cap, KeyServer};
    use std::string;
    use seal::bf_hmac_encryption::parse_encrypted_object;
    use sui::test_scenario::{Self, next_tx, ctx};
    use sui::bls12381::g1_from_bytes;

    let addr1 = @0xA;
    let mut scenario = test_scenario::begin(addr1);

    // Setup key servers.
    let pk0 =
        x"a6b8194ba6ffa1bf4c4e13ab1e56833f99f45f97874e77b845b361305ddaa741174febc307d3e07f7d4d5bb08c0adf3d11a5b8774c84006fb0ba7435f045f56a61905bc283049c2175984528e40a36e0096aabd401a67b1ccc442416c33b5df9";
    let cap0 = register(
        string::utf8(b"mysten0"),
        string::utf8(b"https://mysten-labs.com"),
        0,
        pk0,
        ctx(&mut scenario),
    );
    next_tx(&mut scenario, addr1);
    let s0: KeyServer = test_scenario::take_shared(&scenario);

    let pk1 =
        x"ac1c15fe6c5476ebc8b5bc432dcea06a30c87f89d21b89159ceab06afb84e0e7edefaadb896771ee281d25b6845aa3a20bda9324de39a9909c00f09b344b053da835dfde943c995576ec5e2fcf93221006bb2fcec8ef5096b4b88c36e1aa861c";
    let cap1 = register(
        string::utf8(b"mysten1"),
        string::utf8(b"https://mysten-labs.com"),
        0,
        pk1,
        ctx(&mut scenario),
    );
    next_tx(&mut scenario, addr1);
    let s1: KeyServer = test_scenario::take_shared(&scenario);

    let pk2 =
        x"a8750277f240eb4d94c159b2ec47c1c19396f6e33691fbf50514906b3e70c0454d9a79cf1f1f5562e4ddad9c4505bfb405a9901ac6ba2a51c24919d7599c74a5155f83606f80c1a302de9865deb4577911493dc1608754d67051f755cd44c391";
    let cap2 = register(
        string::utf8(b"mysten2"),
        string::utf8(b"https://mysten-labs.com"),
        0,
        pk2,
        ctx(&mut scenario),
    );
    next_tx(&mut scenario, addr1);
    let s2: KeyServer = test_scenario::take_shared(&scenario);

    // Anyone can create a vote.
    let mut vote = create_vote(
        vector[@0x1, @0x2],
        vector[s0.id().to_inner(), s1.id().to_inner(), s2.id().to_inner()],
        2,
        scenario.ctx(),
    );

    // cargo run --bin seal-cli encrypt-hmac --message 0x07 --aad 0x0000000000000000000000000000000000000000000000000000000000000001 --package-id 0x0 --id 0x381dd9078c322a4663c392761a0211b527c127b29583851217f948d62131f409 --threshold 2 a6b8194ba6ffa1bf4c4e13ab1e56833f99f45f97874e77b845b361305ddaa741174febc307d3e07f7d4d5bb08c0adf3d11a5b8774c84006fb0ba7435f045f56a61905bc283049c2175984528e40a36e0096aabd401a67b1ccc442416c33b5df9 ac1c15fe6c5476ebc8b5bc432dcea06a30c87f89d21b89159ceab06afb84e0e7edefaadb896771ee281d25b6845aa3a20bda9324de39a9909c00f09b344b053da835dfde943c995576ec5e2fcf93221006bb2fcec8ef5096b4b88c36e1aa861c a8750277f240eb4d94c159b2ec47c1c19396f6e33691fbf50514906b3e70c0454d9a79cf1f1f5562e4ddad9c4505bfb405a9901ac6ba2a51c24919d7599c74a5155f83606f80c1a302de9865deb4577911493dc1608754d67051f755cd44c391 -- 0x34401905bebdf8c04f3cd5f04f442a39372c8dc321c29edfb4f9cb30b23ab96 0xd726ecf6f7036ee3557cd6c7b93a49b231070e8eecada9cfa157e40e3f02e5d3 0xdba72804cc9504a82bbaa13ed4a83a0e2c6219d7e45125cf57fd10cbab957a97
    // cargo run --bin seal-cli encrypt-hmac --message 0x2a --aad 0x0000000000000000000000000000000000000000000000000000000000000002 --package-id 0x0 --id 0x381dd9078c322a4663c392761a0211b527c127b29583851217f948d62131f409 --threshold 2 a6b8194ba6ffa1bf4c4e13ab1e56833f99f45f97874e77b845b361305ddaa741174febc307d3e07f7d4d5bb08c0adf3d11a5b8774c84006fb0ba7435f045f56a61905bc283049c2175984528e40a36e0096aabd401a67b1ccc442416c33b5df9 ac1c15fe6c5476ebc8b5bc432dcea06a30c87f89d21b89159ceab06afb84e0e7edefaadb896771ee281d25b6845aa3a20bda9324de39a9909c00f09b344b053da835dfde943c995576ec5e2fcf93221006bb2fcec8ef5096b4b88c36e1aa861c a8750277f240eb4d94c159b2ec47c1c19396f6e33691fbf50514906b3e70c0454d9a79cf1f1f5562e4ddad9c4505bfb405a9901ac6ba2a51c24919d7599c74a5155f83606f80c1a302de9865deb4577911493dc1608754d67051f755cd44c391 -- 0x34401905bebdf8c04f3cd5f04f442a39372c8dc321c29edfb4f9cb30b23ab96 0xd726ecf6f7036ee3557cd6c7b93a49b231070e8eecada9cfa157e40e3f02e5d3 0xdba72804cc9504a82bbaa13ed4a83a0e2c6219d7e45125cf57fd10cbab957a97

    // Cast votes. These have been encrypted using the Seal CLI.
    scenario.next_tx(@0x1);
    let encrypted_vote_1 = parse_encrypted_object(
        x"00000000000000000000000000000000000000000000000000000000000000000020381dd9078c322a4663c392761a0211b527c127b29583851217f948d62131f40903034401905bebdf8c04f3cd5f04f442a39372c8dc321c29edfb4f9cb30b23ab9601d726ecf6f7036ee3557cd6c7b93a49b231070e8eecada9cfa157e40e3f02e5d302dba72804cc9504a82bbaa13ed4a83a0e2c6219d7e45125cf57fd10cbab957a97030200a40f223a7563a06beb4da92ce5b5eb97d477ef72de774d4b87e2a84d4849e658bf372366896fca3844dbab10895d119217e044a74dd890c9644f5d16082ffed3a7e1cb22626371d04e89f6d0d54bebfbe9d9575c3a2e3ec6eb4c751465d70c900369b19b78966d2010a921a6e4bf86abaaac2786b1a80f5601be3953a4d856052b9b923bfb0c4bd6e375918dd6373eaec371d61df4d1cc3582b602c5c4c3013e45ed1a5b433a03cba43ddd458c47e197be31026120aa600b19fd84f1d9bbd0cdacdc8975695398080cb863d3863f4295f3f3347fc0c55cbc6e2eebdb40530858e101012601200000000000000000000000000000000000000000000000000000000000000001032bbebf6451a4a5aef1659dbcd58c4d60e1bf48388f74d7a5cdbd52fb98be97",
    );
    cast_vote(&mut vote, encrypted_vote_1, scenario.ctx());

    scenario.next_tx(@0x2);
    let encrypted_vote_2 = parse_encrypted_object(
        x"00000000000000000000000000000000000000000000000000000000000000000020381dd9078c322a4663c392761a0211b527c127b29583851217f948d62131f40903034401905bebdf8c04f3cd5f04f442a39372c8dc321c29edfb4f9cb30b23ab9601d726ecf6f7036ee3557cd6c7b93a49b231070e8eecada9cfa157e40e3f02e5d302dba72804cc9504a82bbaa13ed4a83a0e2c6219d7e45125cf57fd10cbab957a97030200b9bc3b3f9f5059d30e6999e490b6f8a1d68a92476c9f2a5fbca3e3621775241d9ea881466efb0344e97dd6043f2cf5150dcb4103d97130ddbced1d2feb8c532252bfdfcfbe995a3a3d30f3ba86f169c79ad5c38fbe464d64c3e86f3f7b2fb2e2034cd6ae3eadcbb57cb3201f266e341b8fcc333b13880c16ef81123d55a90c884fc53866532340b0779fe3f99d750fa2d0392e8888728dd690d47462abb085ba8af123a84a61bc56aabe88c63b42cf127356c184b9f2820442475449cb11ab314d03e7342efcb6f852307d94d81362ecbaf36edd863e6c1bbcb740869543ae04150101d4012000000000000000000000000000000000000000000000000000000000000000023fcde4925485dde2bdf7f7db55f47913fa0971498be155cdb9806c1dce8e33f5",
    );
    cast_vote(&mut vote, encrypted_vote_2, scenario.ctx());

    // Both voters have now voted, so the vote is sealed and seal_approve will succeed.
    seal_approve(vote.id(), &vote);

    // The derived keys. These should have been retrieved from key servers
    let dk0 = g1_from_bytes(
        &x"8288e333ba467097dceae2c9bb208712de3f5c6e77cbf7a4b57e3c4a9156a0576949e717cd0ebf46347516ffa424af03",
    );
    let dk1 = g1_from_bytes(
        &x"b307ab62d32189223cef111a150c35d87f037830e39cfcf78583737361ec329b321e1ae3e17a20482a1e6ef388109033",
    );

    // Verify the derived keys
    let user_secret_keys = vector[dk0, dk1];
    let vdks = verify_derived_keys(
        &user_secret_keys,
        @0x0,
        x"381dd9078c322a4663c392761a0211b527c127b29583851217f948d62131f409",
        &vector[get_public_key(&s0), get_public_key(&s1)],
    );

    // Finalize vote
    assert!(vote.result.is_none());
    finalize_vote(
        &mut vote,
        &vdks,
        &vector[get_public_key(&s0), get_public_key(&s1), get_public_key(&s2)],
    );
    assert!(vote.result.is_some());

    // Voter 1 voted '7' and voter 2 voted '42'.
    assert!(vote.result.borrow()[0].borrow() == 7);
    assert!(vote.result.borrow()[1].borrow() == 42);

    // Clean up
    test_scenario::return_shared(s0);
    test_scenario::return_shared(s1);
    test_scenario::return_shared(s2);
    destroy_for_testing(vote);
    destroy_cap(cap0);
    destroy_cap(cap1);
    destroy_cap(cap2);
    test_scenario::end(scenario);
}
