use core::result::Result;

use log::error;

use crate::model::TransactionError;
use crate::model::account::Account;
use crate::model::account::{Available, Held};
use crate::model::transaction::OperationKind;
use crate::model::transaction::{Operation, Transaction, TransactionAction};

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

    pub fn apply(&mut self, transaction: TransactionAction) -> Result<(), TransactionError> {
        if self.suspended {
            return Err(TransactionError::ClientAccountSuspended(self.client_id));
        }
        match transaction {
            TransactionAction::Deposit(transaction_data) => {
                self.available.apply(transaction_data)?;
            }
            TransactionAction::Withdrawal(transaction_data) => {
                self.available.apply(transaction_data)?;
            }
            TransactionAction::Dispute(transaction_reference) => {
                let applied = self
                    .available
                    .transactions
                    .get(&transaction_reference.transaction_id)
                    .ok_or(TransactionError::TransactionNotFound(
                        transaction_reference.transaction_id,
                    ))?;
                if matches!(applied.operation.kind, OperationKind::Debit) {
                    error!(
                        "Cannot dispute withdrawal transaction {}. Not allowed.",
                        transaction_reference.transaction_id.0
                    );
                    return Err(TransactionError::TransactionNotFound(
                        transaction_reference.transaction_id,
                    ));
                }
                let deposit = Transaction {
                    client_id: applied.client_id,
                    transaction_id: applied.transaction_id,
                    operation: Operation::credit(applied.operation.amount),
                };
                let applied = self.held.apply(deposit)?;
                self.available.apply(applied.storno())?;
            }
            TransactionAction::Resolve(transaction_reference) => {
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
            TransactionAction::Chargeback(transaction_reference) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::account;
    use crate::model::transaction::{OperationKind, TransactionId, TransactionReference};
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

    #[derive(Debug, Clone)]
    enum LifecycleStep {
        CreateDeposit { amount: u16 },
        CreateWithdrawal { amount: u16 },
        DisputeExisting,
        ResolveExisting,
        ChargebackExisting,
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

    fn lifecycle_step_strategy() -> impl Strategy<Value = LifecycleStep> {
        prop_oneof![
            (1u16..=10_000).prop_map(|amount| LifecycleStep::CreateDeposit { amount }),
            (1u16..=10_000).prop_map(|amount| LifecycleStep::CreateWithdrawal { amount }),
            Just(LifecycleStep::DisputeExisting),
            Just(LifecycleStep::ResolveExisting),
            Just(LifecycleStep::ChargebackExisting),
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
            .map(|transaction| {
                signed_amount(&transaction.operation.kind, transaction.operation.amount)
            })
            .sum()
    }

    fn assert_balance_invariant(client: &Client) -> proptest::test_runner::TestCaseResult {
        let available_sum = account_transaction_sum(&client.available);
        let held_sum = account_transaction_sum(&client.held);

        prop_assert_eq!(client.available.balance, available_sum);
        prop_assert_eq!(client.held.balance, held_sum);
        prop_assert_eq!(
            client.held.balance + client.available.balance,
            held_sum + available_sum
        );
        Ok(())
    }

    fn into_decimal(amount: u16) -> Decimal {
        Decimal::new(i64::from(amount), 4)
    }

    fn transaction_from_spec(
        client_id: ClientId,
        tx_seed: u16,
        spec: OperationSpec,
    ) -> TransactionAction {
        match spec {
            OperationSpec::Deposit { amount } => TransactionAction::Deposit(Transaction {
                client_id,
                transaction_id: TransactionId::from(u32::from(tx_seed)),
                operation: Operation::credit(into_decimal(amount)),
            }),
            OperationSpec::Withdrawal { amount } => TransactionAction::Withdrawal(Transaction {
                client_id,
                transaction_id: TransactionId::from(u32::from(tx_seed)),
                operation: Operation::debit(into_decimal(amount)),
            }),
            OperationSpec::Dispute { tx } => TransactionAction::Dispute(TransactionReference {
                transaction_id: TransactionId::from(u32::from(tx)),
            }),
            OperationSpec::Resolve { tx } => TransactionAction::Resolve(TransactionReference {
                transaction_id: TransactionId::from(u32::from(tx)),
            }),
            OperationSpec::Chargeback { tx } => {
                TransactionAction::Chargeback(TransactionReference {
                    transaction_id: TransactionId::from(u32::from(tx)),
                })
            }
        }
    }

    fn existing_tx_id(created_tx_ids: &[TransactionId], selector: u16) -> TransactionId {
        let index = usize::from(selector) % created_tx_ids.len();
        created_tx_ids[index]
    }

    #[test]
    fn dispute_moves_funds_from_available_to_held_without_changing_total() {
        let client_id = ClientId(4);
        let tx_id = TransactionId::from(10);
        let amount = Decimal::new(15_000, 4);
        let mut client = Client::new(client_id);

        client
            .apply(TransactionAction::Deposit(Transaction {
                client_id,
                transaction_id: tx_id,
                operation: Operation::credit(amount),
            }))
            .expect("deposit should succeed");

        client
            .apply(TransactionAction::Dispute(TransactionReference {
                transaction_id: tx_id,
            }))
            .expect("dispute should succeed");

        assert_eq!(client.available.balance, Decimal::ZERO);
        assert_eq!(client.held.balance, amount);
        assert_eq!(client.available.balance + client.held.balance, amount);
    }

    #[test]
    fn resolve_restores_available_balance_after_dispute() {
        let client_id = ClientId(5);
        let tx_id = TransactionId::from(11);
        let amount = Decimal::new(20_000, 4);
        let mut client = Client::new(client_id);

        client
            .apply(TransactionAction::Deposit(Transaction {
                client_id,
                transaction_id: tx_id,
                operation: Operation::credit(amount),
            }))
            .expect("deposit should succeed");
        client
            .apply(TransactionAction::Dispute(TransactionReference {
                transaction_id: tx_id,
            }))
            .expect("dispute should succeed");

        client
            .apply(TransactionAction::Resolve(TransactionReference {
                transaction_id: tx_id,
            }))
            .expect("resolve should succeed");

        assert_eq!(client.available.balance, amount);
        assert_eq!(client.held.balance, Decimal::ZERO);
        assert_eq!(client.available.balance + client.held.balance, amount);
    }

    #[test]
    fn chargeback_removes_funds_and_locks_account() {
        let client_id = ClientId(6);
        let tx_id = TransactionId::from(12);
        let amount = Decimal::new(25_000, 4);
        let mut client = Client::new(client_id);

        client
            .apply(TransactionAction::Deposit(Transaction {
                client_id,
                transaction_id: tx_id,
                operation: Operation::credit(amount),
            }))
            .expect("deposit should succeed");
        client
            .apply(TransactionAction::Dispute(TransactionReference {
                transaction_id: tx_id,
            }))
            .expect("dispute should succeed");

        client
            .apply(TransactionAction::Chargeback(TransactionReference {
                transaction_id: tx_id,
            }))
            .expect("chargeback should succeed");

        assert_eq!(client.available.balance, Decimal::ZERO);
        assert_eq!(client.held.balance, Decimal::ZERO);
        assert_eq!(
            client.available.balance + client.held.balance,
            Decimal::ZERO
        );
        assert!(client.suspended);
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
                assert_balance_invariant(&client)?;
            }
        }

        #[test]
        fn keeps_balance_invariant_across_existing_transaction_lifecycles(
            steps in prop::collection::vec((lifecycle_step_strategy(), any::<u16>()), 1..128)
        ) {
            let client_id = ClientId(1);
            let mut client = Client::new(client_id);
            let mut created_tx_ids = Vec::new();

            for (index, (step, selector)) in steps.into_iter().enumerate() {
                let next_tx_id = TransactionId::from(u32::try_from(index + 1).expect("test sequence fits into u32"));
                let transaction = match step {
                    LifecycleStep::CreateDeposit { amount } => {
                        created_tx_ids.push(next_tx_id);
                        TransactionAction::Deposit(Transaction {
                            client_id,
                            transaction_id: next_tx_id,
                            operation: Operation::credit(into_decimal(amount)),
                        })
                    }
                    LifecycleStep::CreateWithdrawal { amount } => {
                        created_tx_ids.push(next_tx_id);
                        TransactionAction::Withdrawal(Transaction {
                            client_id,
                            transaction_id: next_tx_id,
                            operation: Operation::debit(into_decimal(amount)),
                        })
                    }
                    LifecycleStep::DisputeExisting => {
                        let Some(_) = created_tx_ids.first() else {
                            assert_balance_invariant(&client)?;
                            continue;
                        };
                        TransactionAction::Dispute(TransactionReference {
                            transaction_id: existing_tx_id(&created_tx_ids, selector),
                        })
                    }
                    LifecycleStep::ResolveExisting => {
                        let Some(_) = created_tx_ids.first() else {
                            assert_balance_invariant(&client)?;
                            continue;
                        };
                        TransactionAction::Resolve(TransactionReference {
                            transaction_id: existing_tx_id(&created_tx_ids, selector),
                        })
                    }
                    LifecycleStep::ChargebackExisting => {
                        let Some(_) = created_tx_ids.first() else {
                            assert_balance_invariant(&client)?;
                            continue;
                        };
                        TransactionAction::Chargeback(TransactionReference {
                            transaction_id: existing_tx_id(&created_tx_ids, selector),
                        })
                    }
                };
                let _ = client.apply(transaction);
                assert_balance_invariant(&client)?;
            }
        }
    }
}
