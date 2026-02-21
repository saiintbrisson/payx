use rust_decimal::Decimal;
use serde::Deserialize;

/// A transaction type.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransactionType {
    Deposit {
        amount: Decimal,
    },
    Withdrawal {
        amount: Decimal,
    },
    /// Starts a dispute of a transaction.
    ///
    /// [`Transaction::id`] refers to a previous transaction.
    Dispute,
    /// Resolves a transaction, freeing the held balance.
    ///
    /// [`Transaction::id`] refers to a previous transaction.
    Resolve,
    /// Chargebacks a transaction, burning the held balance.
    ///
    /// [`Transaction::id`] refers to a previous transaction.
    Chargeback,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Transaction {
    #[serde(flatten)]
    pub ty: TransactionType,
    #[serde(rename = "client")]
    pub client_id: ClientId,
    /// The transaction ID.
    ///
    /// When this transaction is for a dispute, resolution, or chargeback,
    /// this field's meaning changes and becomes a pointer to a previous
    /// transaction.
    #[serde(rename = "tx")]
    pub id: TransactionId,
}

impl Transaction {
    /// The amount associated with this deposit transaction.
    pub fn deposit_amount(&self) -> Option<Decimal> {
        match self.ty {
            TransactionType::Deposit { amount } => Some(amount),
            _ => None,
        }
    }
}

pub use sealed::{ClientId, TransactionId};

/// Holds newtypes for client and transaction IDs.
///
/// The sealed module is necessary to prevent all modules, including `transaction`
/// itself, from accessing their private fields.
mod sealed {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
    #[serde(transparent)]
    pub struct ClientId(u16);

    impl ClientId {
        pub fn new(id: u16) -> Self {
            Self(id)
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
    #[serde(transparent)]
    pub struct TransactionId(u32);

    impl TransactionId {
        pub fn new(id: u32) -> Self {
            Self(id)
        }
    }
}
