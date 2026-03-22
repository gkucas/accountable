use crate::model::ClientId;
use crate::model::transaction::Operation;
use crate::model::transaction::Transaction;
use crate::model::transaction::TransactionData;
use crate::model::transaction::TransactionReference;
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
pub fn read_transactions(file_name: &str, tx: Sender<Result<Transaction>>) -> Result<()> {
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

fn parse_row(row: &StringRecord, header_info: &HeaderInfo) -> Result<Transaction> {
    let client_id = ClientId(row[header_info.pos_client_id].trim().parse()?);
    let transaction_id = row[header_info.pos_transaction_id]
        .trim()
        .parse::<u32>()?
        .into();
    let transaction = match row[header_info.pos_action]
        .trim()
        .parse::<TransactionType>()?
    {
        TransactionType::Deposit => Transaction::Deposit(TransactionData {
            client_id,
            transaction_id,
            operation: Operation::credit(Decimal::from_str(row[header_info.pos_amount].trim())?),
        }),
        TransactionType::Withdrawal => Transaction::Withdrawal(TransactionData {
            client_id,
            transaction_id,
            operation: Operation::debit(Decimal::from_str(row[header_info.pos_amount].trim())?),
        }),
        TransactionType::Dispute => Transaction::Dispute(TransactionReference {
            client_id,
            transaction_id,
        }),
        TransactionType::Resolve => Transaction::Resolve(TransactionReference {
            client_id,
            transaction_id,
        }),
        TransactionType::Chargeback => Transaction::Chargeback(TransactionReference {
            client_id,
            transaction_id,
        }),
    };
    Ok(transaction)
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
