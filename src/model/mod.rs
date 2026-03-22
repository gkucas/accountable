pub mod account;
pub mod transaction;

use core::{
    fmt::{self, Display},
    result::Result::{self, Ok},
};
use std::collections::HashMap;

use account::Account;
use account::{Available, Held};
use transaction::{Transaction, TransactionId};

pub struct Ledger {
    pub clients: HashMap<ClientId, Client>,
}

impl Ledger {
    pub fn accept(&mut self, transaction: Transaction) -> Result<(), TransactionError> {
        let client = match &transaction {
            Transaction::Deposit(transaction_data) | Transaction::Withdrawal(transaction_data) => {
                self.clients
                    .entry(transaction_data.client_id)
                    .or_insert(Client::new(transaction_data.client_id))
            }
            Transaction::Dispute(transaction_reference)
            | Transaction::Resolve(transaction_reference)
            | Transaction::Chargeback(transaction_reference) => {
                let Some(client) = self.clients.get_mut(&transaction_reference.client_id) else {
                    return Err(TransactionError::ClientDoesNotExist(
                        transaction_reference.client_id,
                    ));
                };
                client
            }
        };
        client.apply(transaction)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u16);

#[derive(Debug)]
pub struct Client {
    pub client_id: ClientId,
    pub available: Account<Available>,
    pub held: Account<Held>,
    pub suspended: bool,
}

impl Client {
    pub fn new(client_id: ClientId) -> Self {
        Self {
            client_id,
            available: Account::new(),
            held: Account::new(),
            suspended: false,
        }
    }
    pub fn apply(&mut self, transaction: Transaction) -> Result<(), TransactionError> {
        if self.suspended {
            return Err(TransactionError::ClientAccountSuspended(self.client_id));
        }
        match transaction {
            Transaction::Deposit(transaction_data) => {
                self.available.apply(transaction_data)?;
            }
            Transaction::Withdrawal(transaction_data) => {
                self.available.apply(transaction_data)?;
            }
            Transaction::Dispute(transaction_reference) => {
                let applied = self
                    .available
                    .transactions
                    .get(&transaction_reference.transaction_id)
                    .ok_or(TransactionError::TransactionNotFound(
                        transaction_reference.transaction_id,
                    ))?;
                let deposit = TransactionData {
                    client_id: applied.client_id,
                    transaction_id: applied.transaction_id,
                    operation: Operation::credit(applied.operation.amount),
                };
                let applied = self.held.apply(deposit)?;
                self.available.apply(applied.storno())?;
            }
            Transaction::Resolve(transaction_reference) => {
                let storno = self
                    .held
                    .transactions
                    .get(&transaction_reference.transaction_id)
                    .ok_or(TransactionError::TransactionNotFound(
                        transaction_reference.transaction_id,
                    ))?
                    .storno();
                let applied = self.held.apply(storno)?;
                self.available.apply(applied.storno())?;
            }
            Transaction::Chargeback(transaction_reference) => {
                let storno = self
                    .held
                    .transactions
                    .get(&transaction_reference.transaction_id)
                    .ok_or(TransactionError::TransactionNotFound(
                        transaction_reference.transaction_id,
                    ))?
                    .storno();
                self.held.apply(storno)?;
                self.suspended = true;
            }
        };
        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::transaction::{
        Operation, OperationKind, Transaction, TransactionData, TransactionId, TransactionReference,
    };
    use proptest::prelude::*;
    use rust_decimal::Decimal;

    #[derive(Debug, Clone)]
    enum OperationSpec {
        Deposit { amount: u16 },
        Withdrawal { amount: u16 },
        Dispute { tx: u16 },
        Resolve { tx: u16 },
        Chargeback { tx: u16 },
    }

    fn operation_spec_strategy() -> impl Strategy<Value = OperationSpec> {
        prop_oneof![
            (1u16..=10_000).prop_map(|amount| OperationSpec::Deposit { amount }),
            (1u16..=10_000).prop_map(|amount| OperationSpec::Withdrawal { amount }),
            (1u16..=64).prop_map(|tx| OperationSpec::Dispute { tx }),
            (1u16..=64).prop_map(|tx| OperationSpec::Resolve { tx }),
            (1u16..=64).prop_map(|tx| OperationSpec::Chargeback { tx }),
        ]
    }

    fn signed_amount(kind: &OperationKind, amount: Decimal) -> Decimal {
        match kind {
            OperationKind::Credit => amount,
            OperationKind::Debit => -amount,
        }
    }

    fn account_transaction_sum<T>(account: &account::Account<T>) -> Decimal {
        account
            .transactions
            .values()
            .map(|transaction| signed_amount(&transaction.operation.kind, transaction.operation.amount))
            .sum()
    }

    fn assert_balance_invariant(client: &Client) {
        let available_sum = account_transaction_sum(&client.available);
        let held_sum = account_transaction_sum(&client.held);

        prop_assert_eq!(client.available.balance, available_sum);
        prop_assert_eq!(client.held.balance, held_sum);
        prop_assert_eq!(
            client.held.balance + client.available.balance,
            held_sum + available_sum
        );
    }

    fn into_decimal(amount: u16) -> Decimal {
        Decimal::new(i64::from(amount), 4)
    }

    fn transaction_from_spec(client_id: ClientId, tx_seed: u16, spec: OperationSpec) -> Transaction {
        match spec {
            OperationSpec::Deposit { amount } => Transaction::Deposit(TransactionData {
                client_id,
                transaction_id: TransactionId::from(u32::from(tx_seed)),
                operation: Operation::credit(into_decimal(amount)),
            }),
            OperationSpec::Withdrawal { amount } => Transaction::Withdrawal(TransactionData {
                client_id,
                transaction_id: TransactionId::from(u32::from(tx_seed)),
                operation: Operation::debit(into_decimal(amount)),
            }),
            OperationSpec::Dispute { tx } => Transaction::Dispute(TransactionReference {
                client_id,
                transaction_id: TransactionId::from(u32::from(tx)),
            }),
            OperationSpec::Resolve { tx } => Transaction::Resolve(TransactionReference {
                client_id,
                transaction_id: TransactionId::from(u32::from(tx)),
            }),
            OperationSpec::Chargeback { tx } => Transaction::Chargeback(TransactionReference {
                client_id,
                transaction_id: TransactionId::from(u32::from(tx)),
            }),
        }
    }

    proptest! {
        #[test]
        fn keeps_balance_invariant(operations in prop::collection::vec(operation_spec_strategy(), 1..128)) {
            let client_id = ClientId(1);
            let mut client = Client::new(client_id);

            for (index, spec) in operations.into_iter().enumerate() {
                let tx_seed = u16::try_from(index + 1).expect("generated sequence fits into u16");
                let transaction = transaction_from_spec(client_id, tx_seed, spec);
                let _ = client.apply(transaction);
                assert_balance_invariant(&client);
            }
        }
    }
}
