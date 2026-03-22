use core::result::Result;
use std::{collections::HashMap, env};

use anyhow::Error;
use log::error;

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
                Ok(transaction) => match ledger.accept(transaction) {
                    Result::Ok(_) => {}
                    Result::Err(err) => {
                        error!("Cannot apply transaction, skipping. Error: {}", err)
                    }
                },
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
