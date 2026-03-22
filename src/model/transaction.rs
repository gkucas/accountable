use core::{
    convert::From,
    result::Result::{self, Ok},
};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::model::{ClientId, TransactionError, account::Account};

pub const UUID_NAMESPACE: Uuid = Uuid::from_bytes([
    0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x12, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17, 0x40, 0x00,
]);

impl TransactionId {
    pub fn child(&self) -> Self {
        TransactionId(Uuid::new_v5(&UUID_NAMESPACE, self.0.as_bytes()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransactionId(pub Uuid);

impl From<u32> for TransactionId {
    fn from(value: u32) -> Self {
        Self(Uuid::new_v5(&UUID_NAMESPACE, &value.to_be_bytes()))
    }
}

#[derive(Debug, Clone)]
pub struct TransactionData<Status> {
    pub client_id: ClientId,
    pub transaction_id: TransactionId,
    pub operation: Operation<Status>,
}

#[derive(Debug, Clone)]
pub struct Operation<Status> {
    pub kind: OperationKind,
    pub amount: Decimal,
    _status: std::marker::PhantomData<Status>,
}

#[derive(Debug, Clone)]
pub enum OperationKind {
    Debit,
    Credit,
}

impl<Status> Operation<Status> {
    pub fn debit(amount: Decimal) -> Self {
        Operation {
            kind: OperationKind::Debit,
            amount,
            _status: std::marker::PhantomData,
        }
    }
    pub fn credit(amount: Decimal) -> Self {
        Operation {
            kind: OperationKind::Credit,
            amount,
            _status: std::marker::PhantomData,
        }
    }
}

impl Operation<Applied> {
    pub fn storno(&self) -> Operation<Pending> {
        match self.kind {
            OperationKind::Debit => Operation::credit(self.amount),
            OperationKind::Credit => Operation::debit(self.amount),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Applied {}
#[derive(Debug, Clone)]
pub struct Pending {}

impl TransactionData<Pending> {
    pub fn apply<T>(
        self,
        account: &mut Account<T>,
    ) -> Result<TransactionData<Applied>, TransactionError> {
        let amount = self.operation.amount;
        let operation = match self.operation.kind {
            OperationKind::Debit => {
                if account.balance < amount {
                    return Err(TransactionError::InsufficientFunds(self.client_id));
                }
                account.balance -= amount;
                Operation::debit(amount)
            }
            OperationKind::Credit => {
                account.balance += amount;
                Operation::credit(amount)
            }
        };
        Ok(TransactionData {
            client_id: self.client_id,
            transaction_id: self.transaction_id,
            operation,
        })
    }
}

impl TransactionData<Applied> {
    pub fn storno(&self) -> TransactionData<Pending> {
        TransactionData {
            client_id: self.client_id,
            transaction_id: self.transaction_id.child(),
            operation: self.operation.storno(),
        }
    }
}

#[derive(Debug)]
pub struct TransactionReference {
    pub client_id: ClientId,
    pub transaction_id: TransactionId,
}

#[derive(Debug)]
pub enum Transaction {
    Deposit(TransactionData<Pending>),
    Withdrawal(TransactionData<Pending>),
    Dispute(TransactionReference),
    Resolve(TransactionReference),
    Chargeback(TransactionReference),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn storno_reverses_credit_operation_into_debit() {
        let operation = Operation::<Applied>::credit(Decimal::new(12_500, 4));

        let storno = operation.storno();

        assert!(matches!(storno.kind, OperationKind::Debit));
        assert_eq!(storno.amount, Decimal::new(12_500, 4));
    }

    #[test]
    fn storno_reverses_debit_operation_into_credit() {
        let operation = Operation::<Applied>::debit(Decimal::new(40_001, 4));

        let storno = operation.storno();

        assert!(matches!(storno.kind, OperationKind::Credit));
        assert_eq!(storno.amount, Decimal::new(40_001, 4));
    }

    #[test]
    fn transaction_data_storno_preserves_client_and_amount_and_creates_child_tx_id() {
        let original = TransactionData::<Applied> {
            client_id: ClientId(7),
            transaction_id: TransactionId::from(42),
            operation: Operation::credit(Decimal::new(15, 1)),
        };

        let storno = original.storno();

        assert_eq!(storno.client_id, original.client_id);
        assert_ne!(storno.transaction_id, original.transaction_id);
        assert_eq!(storno.transaction_id, original.transaction_id.child());
        assert!(matches!(storno.operation.kind, OperationKind::Debit));
        assert_eq!(storno.operation.amount, original.operation.amount);
    }

    #[test]
    fn child_transaction_id_is_deterministic_for_the_same_parent() {
        let parent = TransactionId::from(42);

        let first_child = parent.child();
        let second_child = parent.child();

        assert_eq!(first_child, second_child);
        assert_ne!(first_child, parent);
    }

    #[test]
    fn different_parents_produce_different_child_transaction_ids() {
        let first_parent = TransactionId::from(41);
        let second_parent = TransactionId::from(42);

        let first_child = first_parent.child();
        let second_child = second_parent.child();

        assert_ne!(first_parent, second_parent);
        assert_ne!(first_child, second_child);
    }
}
