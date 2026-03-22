use core::result::Result;
use std::collections::HashMap;

use log::error;

use crate::model::transaction::{TransactionAction, TransactionSubmission};
use crate::model::{Client, ClientId, TransactionError};

pub struct Ledger {
    pub clients: HashMap<ClientId, Client>,
    pub incoming_transactions: tokio::sync::mpsc::Receiver<TransactionSubmission>,
}

impl Ledger {
    pub async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            match self.incoming_transactions.recv().await {
                Some(transaction) => match self.accept(transaction) {
                    Result::Ok(_) => {}
                    Result::Err(err) => {
                        error!("Cannot apply transaction, skipping. Error: {}", err)
                    }
                },
                None => break,
            }
        }
        Ok(())
    }

    pub fn accept(&mut self, transaction: TransactionSubmission) -> Result<(), TransactionError> {
        let client = match &transaction.action {
            TransactionAction::Deposit(_) | TransactionAction::Withdrawal(_) => self
                .clients
                .entry(transaction.client_id)
                .or_insert(Client::new(transaction.client_id)),
            TransactionAction::Dispute(_)
            | TransactionAction::Resolve(_)
            | TransactionAction::Chargeback(_) => {
                let Some(client) = self.clients.get_mut(&transaction.client_id) else {
                    return Err(TransactionError::ClientDoesNotExist(transaction.client_id));
                };
                client
            }
        };
        client.apply(transaction.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::transaction::{
        Operation, Transaction, TransactionAction, TransactionId, TransactionReference,
        TransactionSubmission,
    };
    use crate::model::{ClientId, TransactionError};
    use rust_decimal::Decimal;

    fn submission(client_id: ClientId, action: TransactionAction) -> TransactionSubmission {
        TransactionSubmission { client_id, action }
    }

    fn test_ledger() -> Ledger {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ledger {
            clients: HashMap::new(),
            incoming_transactions: rx,
        }
    }

    #[test]
    fn ledger_creates_client_for_deposit() {
        let client_id = ClientId(1);
        let mut ledger = test_ledger();

        let result = ledger.accept(submission(
            client_id,
            TransactionAction::Deposit(Transaction {
                client_id,
                transaction_id: TransactionId::from(1),
                operation: Operation::credit(Decimal::new(10_000, 4)),
            }),
        ));

        assert!(result.is_ok());
        assert!(ledger.clients.contains_key(&client_id));
    }

    #[test]
    fn ledger_creates_client_for_withdrawal() {
        let client_id = ClientId(2);
        let mut ledger = test_ledger();

        let result = ledger.accept(submission(
            client_id,
            TransactionAction::Withdrawal(Transaction {
                client_id,
                transaction_id: TransactionId::from(2),
                operation: Operation::debit(Decimal::new(10_000, 4)),
            }),
        ));

        assert!(ledger.clients.contains_key(&client_id));
        assert!(matches!(result, Err(TransactionError::InsufficientFunds(id)) if id == client_id));
    }

    #[test]
    fn ledger_does_not_create_client_for_missing_reference_transaction() {
        let client_id = ClientId(3);
        let mut ledger = test_ledger();

        let result = ledger.accept(submission(
            client_id,
            TransactionAction::Dispute(TransactionReference {
                transaction_id: TransactionId::from(3),
            }),
        ));

        assert!(matches!(result, Err(TransactionError::ClientDoesNotExist(id)) if id == client_id));
        assert!(!ledger.clients.contains_key(&client_id));
    }
}
