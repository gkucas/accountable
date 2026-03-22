use std::{collections::HashMap, env};

use anyhow::Error;
use log::error;

use crate::model::Client;
use crate::model::Ledger;

mod model;
mod reader;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let file_name = env::args().nth(1).expect("Please provide file name");
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    reader::read_transactions(&file_name, tx)?;

    let mut ledger = Ledger {
        clients: HashMap::new(),
    };

    loop {
        match rx.recv().await {
            None => break,
            Some(tx) => match tx {
                Ok(transaction) => {
                    let client = match &transaction {
                        model::transaction::Transaction::Deposit(transaction_data)
                        | model::transaction::Transaction::Withdrawal(transaction_data) => ledger
                            .clients
                            .entry(transaction_data.client_id)
                            .or_insert(Client::new(transaction_data.client_id)),
                        model::transaction::Transaction::Dispute(transaction_reference)
                        | model::transaction::Transaction::Resolve(transaction_reference)
                        | model::transaction::Transaction::Chargeback(transaction_reference) => {
                            let Some(client) =
                                ledger.clients.get_mut(&transaction_reference.client_id)
                            else {
                                error!(
                                    "Cannot apply transaction {}, client {} does not exist",
                                    transaction_reference.transaction_id.0,
                                    transaction_reference.client_id.0
                                );
                                continue;
                            };
                            client
                        }
                    };
                    match client.apply(transaction) {
                        core::result::Result::Ok(_) => {}
                        core::result::Result::Err(err) => {
                            error!("Cannot apply transaction, skipping. Error: {}", err)
                        }
                    }
                }
                Err(err) => {
                    error!("Stopping file processing, error: {}", err);
                    return Err(err);
                }
            },
        }
    }
    println!("client, available, held, total, locked");
    for client in ledger.clients.values() {
        let available = client.available.balance;
        let held = client.held.balance;
        println!(
            "{},{},{},{},{}",
            client.client_id.0,
            available,
            held,
            available + held,
            client.suspended
        );
    }
    Ok(())
}
