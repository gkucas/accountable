use crate::model::transaction::Pending;
use crate::model::{
    TransactionError,
    transaction::{Applied, Transaction, TransactionId},
};
use core::{
    marker::PhantomData,
    result::Result::{self, Ok},
};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Debug)]
pub struct Available;

#[derive(Debug)]
pub struct Held;

#[derive(Debug)]
pub struct Account<Type> {
    pub balance: Decimal,
    pub transactions: HashMap<TransactionId, Transaction<Applied>>,
    _status: std::marker::PhantomData<Type>,
}

impl<Type> Account<Type> {
    pub fn new() -> Self {
        Self {
            balance: Decimal::ZERO,
            transactions: HashMap::new(),
            _status: PhantomData,
        }
    }
}

impl<Type> Account<Type> {
    pub fn apply(
        &mut self,
        data: Transaction<Pending>,
    ) -> Result<Transaction<Applied>, TransactionError> {
        let tx_id = data.transaction_id;
        if self.transactions.contains_key(&tx_id) {
            return Err(TransactionError::DuplicateTransaction(tx_id));
        }
        let applied = data.apply(self)?;
        if self.transactions.insert(tx_id, applied.clone()).is_some() {
            return Err(TransactionError::DuplicateTransaction(tx_id));
        }
        Ok(applied)
    }
}
