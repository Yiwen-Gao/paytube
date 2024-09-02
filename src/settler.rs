//! PayTube's "settler" component for settling the final ledgers across all
//! channel participants.
//!
//! When users are finished transacting, the resulting ledger is used to craft
//! a batch of transactions to settle all state changes to the base chain
//! (Solana).
//!
//! The interesting piece here is that there can be hundreds or thousands of
//! transactions across a handful of users, but only the resulting difference
//! between their balance when the channel opened and their balance when the
//! channel is about to close are needed to create the settlement transaction.

use {
    crate::transaction::PayTubeTransaction,
    solana_client::rpc_client::RpcClient,
    solana_sdk::{
        instruction::Instruction as SolanaInstruction, pubkey::Pubkey, signature::Keypair,
        signer::Signer, system_instruction, transaction::Transaction as SolanaTransaction,
        signature::Signature,
    },
    solana_transaction_status::UiTransactionEncoding,
    solana_svm::transaction_processor::LoadAndExecuteSanitizedTransactionsOutput,
    spl_associated_token_account::get_associated_token_address,
    std::collections::HashMap,
    std::sync::Arc,
    tokio::{task, join},
    futures::future::join_all,
};

/// The key used for storing ledger entries.
///
/// Each entry in the ledger represents the movement of SOL or tokens between
/// two parties. The two keys of the two parties are stored in a sorted array
/// of length two, and the value's sign determines the direction of transfer.
///
/// This design allows the ledger to combine transfers from a -> b and b -> a
/// in the same entry, calculating the final delta between two parties.
#[derive(PartialEq, Eq, Hash)]
struct LedgerKey {
    mint: Option<Pubkey>,
    keys: [Pubkey; 2],
}

/// A ledger of PayTube transactions, used to deconstruct into base chain
/// transactions.
///
/// The value is stored as a signed `i128`, in order to include a sign but also
/// provide enough room to store `u64::MAX`.
struct Ledger {
    ledger: HashMap<LedgerKey, i128>,
}

impl Ledger {
    fn new(
        paytube_transactions: &[PayTubeTransaction],
        svm_output: LoadAndExecuteSanitizedTransactionsOutput,
    ) -> Self {
        let mut ledger: HashMap<LedgerKey, i128> = HashMap::new();
        paytube_transactions
            .iter()
            .zip(svm_output.execution_results)
            .for_each(|(transaction, result)| {
                // Only append to the ledger if the PayTube transaction was
                // successful.
                if result.was_executed_successfully() {
                    let mint = transaction.mint;
                    let mut keys = [transaction.from, transaction.to];
                    let amount = transaction.amount as i128;
                    keys.sort();
                    let amount = if keys.iter().position(|k| k.eq(&transaction.from)).unwrap() == 0
                    {
                        transaction.amount as i128
                    } else {
                        -(transaction.amount as i128)
                    };
                    *ledger.entry(LedgerKey { mint, keys }).or_default() += amount;
                }
            });
        Self { ledger }
    }

    fn generate_base_chain_instructions(&self) -> Vec<SolanaInstruction> {
        self.ledger
            .iter()
            .map(|(key, amount)| {
                let (from, to, amount) = if *amount < 0 {
                    (key.keys[1], key.keys[0], (amount * -1) as u64)
                } else {
                    (key.keys[0], key.keys[1], *amount as u64)
                };
                if let Some(mint) = key.mint {
                    let source_pubkey = get_associated_token_address(&from, &mint);
                    let destination_pubkey = get_associated_token_address(&to, &mint);
                    return spl_token::instruction::transfer(
                        &spl_token::id(),
                        &source_pubkey,
                        &destination_pubkey,
                        &from,
                        &[],
                        amount,
                    )
                    .unwrap();
                }
                system_instruction::transfer(&from, &to, amount)
            })
            .collect::<Vec<_>>()
    }
}

/// PayTube final transaction settler.
pub struct PayTubeSettler/*<'a>*/ {
    rpc_client: Arc<RpcClient>, // &'a RpcClient,
}

impl/*<'a>*/ PayTubeSettler/*<'a>*/ {
    pub fn new(rpc_client: Arc<RpcClient> /*&'a RpcClient*/) -> Self {
        Self { rpc_client }
    }

    /// Settle the payment channel results to the Solana blockchain.
    pub async fn process_settle(
        &self,
        paytube_transactions: &[PayTubeTransaction],
        svm_output: LoadAndExecuteSanitizedTransactionsOutput,
        keys: &[Keypair],
    ) {
        // // Build the ledger from the processed PayTube transactions.
        // let ledger = Ledger::new(paytube_transactions, svm_output);
        //
        // // Build the Solana instructions from the ledger.
        // let instructions = ledger.generate_base_chain_instructions();

        // Send the transactions to the Solana blockchain.
        let recent_blockhash = self.rpc_client.get_latest_blockhash().unwrap();
        // instructions.chunks(10).for_each(|chunk| {
        //     let transaction = SolanaTransaction::new_signed_with_payer(
        //         chunk,
        //         Some(&keys[0].pubkey()),
        //         keys,
        //         recent_blockhash,
        //     );
        //     self.rpc_client
        //         .send_and_confirm_transaction(&transaction)
        //         .unwrap();
        // });

        // part 1
        let [payer, alice, bob, will, ..] = keys else { todo!() };
        let ins1 = [
            system_instruction::transfer(&alice.pubkey(), &bob.pubkey(), 2_000_000),
            system_instruction::transfer(&bob.pubkey(), &will.pubkey(), 5_000_000),
        ];
        let tx1 = SolanaTransaction::new_signed_with_payer(
            &ins1,
            Some(&keys[0].pubkey()),
            &[payer, alice, bob],
            recent_blockhash,
        );

        // part 2
        let recent_blockhash = self.rpc_client.get_latest_blockhash().unwrap();
        let ins2 = [
            system_instruction::transfer(&alice.pubkey(), &bob.pubkey(), 2_000_000),
            system_instruction::transfer(&will.pubkey(), &alice.pubkey(), 1_000_000),
        ];
        let tx2 = SolanaTransaction::new_signed_with_payer(
            &ins2,
            Some(&keys[0].pubkey()),
            &[payer, alice, will],
            recent_blockhash,
        );

        let txs = [tx1, tx2];
        let [future1, future2] = txs.map(|transaction| {
            let rpc_client = self.rpc_client.clone();
            tokio::spawn(async move {
                rpc_client
                    .send_and_confirm_transaction(&transaction)
                    .unwrap()
            })
        });

        // alice: 8_000_000 (-2M)
        // bob: 7_000_000 (+2M - 5M)
        // will: 15_000_000 (+5M)

        // alice: 9_000_000 (-2M + 1M)
        // bob: 12_000_000 (+2M)
        // will: 9_000_000 (-1M)

        // let res = join!(future1, future2);
        // futures::future::join_all([&future1, &future2]).await;
        let res1: Signature = future1.await.unwrap();
        // let status1 = self.rpc_client.get_transaction(
        //     &res1,
        //     UiTransactionEncoding::Json,
        // ).unwrap();
        let res2: Signature = future2.await.unwrap();
        // let status2 = self.rpc_client.get_transaction(
        //     &res2,
        //     UiTransactionEncoding::Json,
        // ).unwrap();
        // println!("{:?} {:?}", status1.slot, status2.slot)
    }
}
