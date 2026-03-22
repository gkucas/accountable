use crate::model::transaction::Transaction;
use crate::model::transaction::TransactionAction;
use crate::model::transaction::TransactionReference;
use crate::model::transaction::TransactionSubmission;
use crate::model::{ClientId, transaction::Operation};
use anyhow::Result;
use anyhow::anyhow;
use csv::StringRecord;
use rust_decimal::Decimal;
use std::str::FromStr;
use strum_macros::EnumString;
use tokio::sync::mpsc::Sender;

struct HeaderInfo {
    pos_action: usize,
    pos_client_id: usize,
    pos_transaction_id: usize,
    pos_amount: usize,
}
pub fn read_transactions(file_name: &str, tx: Sender<Result<TransactionSubmission>>) -> Result<()> {
    let mut reader = csv::Reader::from_path(file_name)?;
    let headers = reader.headers()?;
    let mut header_info = HeaderInfo {
        pos_action: 0,
        pos_client_id: 0,
        pos_transaction_id: 0,
        pos_amount: 0,
    };
    let mut matches = 0;
    for (index, header) in headers.iter().enumerate() {
        match header.trim() {
            "type" => {
                header_info.pos_action = index;
                matches += 1;
            }
            "client" => {
                header_info.pos_client_id = index;
                matches += 1;
            }
            "tx" => {
                header_info.pos_transaction_id = index;
                matches += 1;
            }
            "amount" => {
                header_info.pos_amount = index;
                matches += 1;
            }
            header => return Err(anyhow!("Found unxpected header: {}", header)),
        }
    }

    if matches < 4 {
        return Err(anyhow!("Missing headers expected 4 found {}", matches));
    }

    tokio::task::spawn_blocking(move || {
        for row in reader.records() {
            let result = match row {
                Ok(row) => tx.blocking_send(parse_row(&row, &header_info).map_err(|err| {
                    anyhow!(
                        "Cannot read file row {:?}. Content: {}, Error: {}",
                        row.position(),
                        row.iter().collect::<Vec<_>>().join(","),
                        err
                    )
                })),
                Err(err) => tx.blocking_send(Err(anyhow!(err))),
            };
            match result {
                Ok(_) => (),
                Err(err) => log::error!(
                    "Stopping file reading. Failed sending message to channel error: {:?}",
                    err
                ),
            }
        }
    });
    Ok(())
}

fn parse_row(row: &StringRecord, header_info: &HeaderInfo) -> Result<TransactionSubmission> {
    let client_id = ClientId(row[header_info.pos_client_id].trim().parse()?);
    let transaction_id = row[header_info.pos_transaction_id]
        .trim()
        .parse::<u32>()?
        .into();
    let action = match row[header_info.pos_action]
        .trim()
        .parse::<TransactionType>()?
    {
        TransactionType::Deposit => TransactionAction::Deposit(Transaction {
            client_id,
            transaction_id,
            operation: Operation::credit(Decimal::from_str(row[header_info.pos_amount].trim())?),
        }),
        TransactionType::Withdrawal => TransactionAction::Withdrawal(Transaction {
            client_id,
            transaction_id,
            operation: Operation::debit(Decimal::from_str(row[header_info.pos_amount].trim())?),
        }),
        TransactionType::Dispute => {
            TransactionAction::Dispute(TransactionReference { transaction_id })
        }
        TransactionType::Resolve => {
            TransactionAction::Resolve(TransactionReference { transaction_id })
        }
        TransactionType::Chargeback => {
            TransactionAction::Chargeback(TransactionReference { transaction_id })
        }
    };
    Ok(TransactionSubmission { client_id, action })
}

#[derive(Debug, EnumString)]
#[strum(serialize_all = "lowercase")]
enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::transaction::{OperationKind, TransactionId};
    use csv::StringRecord;
    use rust_decimal::Decimal;

    fn header_info() -> HeaderInfo {
        HeaderInfo {
            pos_action: 0,
            pos_client_id: 1,
            pos_transaction_id: 2,
            pos_amount: 3,
        }
    }

    #[test]
    fn parses_deposit_with_whitespace() {
        let row = StringRecord::from(vec!["deposit", " 1 ", " 2 ", " 1.2500 "]);

        let transaction = parse_row(&row, &header_info()).expect("deposit should parse");

        assert_eq!(transaction.client_id.0, 1);
        match transaction.action {
            TransactionAction::Deposit(data) => {
                assert_eq!(data.client_id.0, 1);
                assert_eq!(data.transaction_id, TransactionId::from(2));
                assert!(matches!(data.operation.kind, OperationKind::Credit));
                assert_eq!(data.operation.amount, Decimal::new(12_500, 4));
            }
            other => panic!("expected deposit, got {:?}", other),
        }
    }

    #[test]
    fn parses_withdrawal_with_whitespace() {
        let row = StringRecord::from(vec!["withdrawal", " 7 ", " 9 ", " 4.0001 "]);

        let transaction = parse_row(&row, &header_info()).expect("withdrawal should parse");

        assert_eq!(transaction.client_id.0, 7);
        match transaction.action {
            TransactionAction::Withdrawal(data) => {
                assert_eq!(data.client_id.0, 7);
                assert_eq!(data.transaction_id, TransactionId::from(9));
                assert!(matches!(data.operation.kind, OperationKind::Debit));
                assert_eq!(data.operation.amount, Decimal::new(40_001, 4));
            }
            other => panic!("expected withdrawal, got {:?}", other),
        }
    }

    #[test]
    fn parses_dispute_without_amount() {
        let row = StringRecord::from(vec!["dispute", "1", "3", ""]);

        let transaction = parse_row(&row, &header_info()).expect("dispute should parse");

        assert_eq!(transaction.client_id.0, 1);
        match transaction.action {
            TransactionAction::Dispute(reference) => {
                assert_eq!(reference.transaction_id, TransactionId::from(3));
            }
            other => panic!("expected dispute, got {:?}", other),
        }
    }

    #[test]
    fn parses_resolve_without_amount() {
        let row = StringRecord::from(vec!["resolve", "1", "3", ""]);

        let transaction = parse_row(&row, &header_info()).expect("resolve should parse");

        assert_eq!(transaction.client_id.0, 1);
        match transaction.action {
            TransactionAction::Resolve(reference) => {
                assert_eq!(reference.transaction_id, TransactionId::from(3));
            }
            other => panic!("expected resolve, got {:?}", other),
        }
    }

    #[test]
    fn parses_chargeback_without_amount() {
        let row = StringRecord::from(vec!["chargeback", "1", "3", ""]);

        let transaction = parse_row(&row, &header_info()).expect("chargeback should parse");

        assert_eq!(transaction.client_id.0, 1);
        match transaction.action {
            TransactionAction::Chargeback(reference) => {
                assert_eq!(reference.transaction_id, TransactionId::from(3));
            }
            other => panic!("expected chargeback, got {:?}", other),
        }
    }

    #[test]
    fn rejects_deposit_with_missing_amount() {
        let row = StringRecord::from(vec!["deposit", "1", "1", ""]);

        let error =
            parse_row(&row, &header_info()).expect_err("deposit without amount should fail");

        assert!(error.to_string().contains("Invalid decimal"));
    }

    #[test]
    fn rejects_unknown_transaction_type() {
        let row = StringRecord::from(vec!["refund", "1", "1", "1.0"]);

        let error =
            parse_row(&row, &header_info()).expect_err("unknown transaction type should fail");

        assert!(
            !error.to_string().is_empty(),
            "parsing an unknown transaction type should return an error"
        );
    }
}
