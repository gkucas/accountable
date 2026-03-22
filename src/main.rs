use core::result::Result;
use std::{collections::HashMap, env};

use anyhow::Error;
use log::error;
use tokio::sync::mpsc::{Sender, channel};
use tokio::task::JoinHandle;

use crate::model::{Ledger, transaction::TransactionSubmission};

mod model;
mod reader;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let file_name = env::args().nth(1).expect("Please provide file name");
    let cores = num_cpus::get();
    let mut senders: Vec<Sender<TransactionSubmission>> = Vec::with_capacity(cores);
    let mut handles: Vec<JoinHandle<anyhow::Result<Ledger>>> = Vec::with_capacity(cores);

    for _ in 0..cores {
        let (tx, rx) = channel(100);
        let mut ledger = Ledger {
            clients: HashMap::new(),
            incoming_transactions: rx,
        };
        let handle = tokio::spawn(async move {
            ledger.run().await?;
            Ok(ledger)
        });
        handles.push(handle);
        senders.push(tx);
    }
    let (tx, mut rx) = channel(100);
    reader::read_transactions(&file_name, tx)?;
    loop {
        match rx.recv().await {
            None => break,
            Some(tx) => match tx {
                Ok(submission) => {
                    let partition = (submission.client_id.0 as usize) % cores;
                    senders[partition].send(submission).await?;
                }
                Err(err) => {
                    error!("Stopping file processing, error: {}", err);
                    return Err(err);
                }
            },
        }
    }
    drop(senders);

    println!("client, available, held, total, locked");
    for handle in handles {
        let ledger = handle.await??;
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
    }
    Ok(())
}
