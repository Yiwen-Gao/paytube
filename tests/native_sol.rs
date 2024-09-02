mod setup;

use {
    paytube_svm::{transaction::PayTubeTransaction, PayTubeChannel},
    setup::{system_account, TestValidatorContext},
    solana_sdk::{signature::Keypair, signer::Signer},
    solana_sdk::signer::keypair::keypair_from_seed,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_native_sol() {
    let seed: &[u8; 32] = b"an_example_fixed_seed_for_testin";
    // Generate a fixed keypair using the seed
    let alice = keypair_from_seed(seed).unwrap();
    let seed: &[u8; 32] = b"an_example_fixed_seed_for_testim";
    // Generate a fixed keypair using the seed
    let bob = keypair_from_seed(seed).unwrap();
    let seed: &[u8; 32] = b"an_example_fixed_seed_for_testio";
    // Generate a fixed keypair using the seed
    let will = keypair_from_seed(seed).unwrap();
    // let alice = // Keypair::new();
    // let bob = Keypair::new();
    // let will = Keypair::new();

    let alice_pubkey = alice.pubkey();
    let bob_pubkey = bob.pubkey();
    let will_pubkey = will.pubkey();

    let accounts = vec![
        (alice_pubkey, system_account(10_000_000)),
        (bob_pubkey, system_account(10_000_000)),
        (will_pubkey, system_account(10_000_000)),
    ];

    let context = tokio::task::spawn_blocking(|| {
        TestValidatorContext::start_with_accounts(accounts)
    }).await.expect("Failed to setup test validator");
    let test_validator = &context.test_validator;
    let payer = context.payer.insecure_clone();
    let payer_pubkey = payer.pubkey();

    let rpc_client = test_validator.get_rpc_client();

    let paytube_channel = PayTubeChannel::new(vec![payer, alice, bob, will], rpc_client);

    paytube_channel.process_paytube_transfers(&[
        // Alice -> Bob 2_000_000
        PayTubeTransaction {
            from: alice_pubkey,
            to: bob_pubkey,
            amount: 2_000_000,
            mint: None,
        },
        // Bob -> Will 5_000_000
        PayTubeTransaction {
            from: bob_pubkey,
            to: will_pubkey,
            amount: 5_000_000,
            mint: None,
        },
        // Alice -> Bob 2_000_000
        PayTubeTransaction {
            from: alice_pubkey,
            to: bob_pubkey,
            amount: 2_000_000,
            mint: None,
        },
        // Will -> Alice 1_000_000
        PayTubeTransaction {
            from: will_pubkey,
            to: alice_pubkey,
            amount: 1_000_000,
            mint: None,
        },
    ]).await;

    // Ledger:
    // Alice:   10_000_000 - 2_000_000 - 2_000_000 + 1_000_000  = 7_000_000
    // Bob:     10_000_000 + 2_000_000 - 5_000_000 + 2_000_000  = 9_000_000
    // Will:    10_000_000 + 5_000_000 - 1_000_000              = 14_000_000
    let rpc_client = test_validator.get_rpc_client();
    let alice_balance = rpc_client.get_balance(&alice_pubkey).unwrap();
    assert_eq!(alice_balance, 7_000_000);
    let bob_balance = rpc_client.get_balance(&bob_pubkey).unwrap();
    assert_eq!(bob_balance, 9_000_000);
    let will_balance = rpc_client.get_balance(&will_pubkey).unwrap();
    assert_eq!(will_balance, 14_000_000);

    // alice 154
    // bob 53
    // will 135
    // payer 33
}
