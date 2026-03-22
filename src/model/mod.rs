pub mod account;
pub mod client;
pub mod ledger;
pub mod transaction;

use core::fmt::{self, Display};
pub use client::{Client, ClientId};
pub use ledger::Ledger;
use transaction::TransactionId;

#[derive(Debug)]
pub enum TransactionError {
    InsufficientFunds(ClientId),
    ClientDoesNotExist(ClientId),
    TransactionNotFound(TransactionId),
    DuplicateTransaction(TransactionId),
    ClientAccountSuspended(ClientId),
}

impl Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionError::InsufficientFunds(client_id) => {
                write!(f, "Insufficient funds for client {:?}", client_id)
            }
            TransactionError::TransactionNotFound(tx_id) => {
                write!(f, "Transaction not found: {}", tx_id.0)
            }
            TransactionError::DuplicateTransaction(tx_id) => {
                write!(f, "Duplicate transaction: {}", tx_id.0)
            }
            TransactionError::ClientAccountSuspended(client_id) => {
                write!(f, "Client account is suspended: {}", client_id.0)
            }
            TransactionError::ClientDoesNotExist(client_id) => {
                write!(f, "Client does not exist: {}", client_id.0)
            }
        }
    }
}
