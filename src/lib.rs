use std::path::Path;

use indexmap::IndexMap;

use crate::{
    client::{ClientAccount, TransactionError},
    transaction::{ClientId, Transaction},
};

pub mod client;
pub mod transaction;

/// A collection of clients.
///
/// **NOTE:** The main reason I decided to structure it like
/// this was to allow for easier testing reading from CSVs
/// without having to duplicate the logic here.
///
/// Another note, because client ID is a u16, this could be
/// turned into a Vec or even better, a boxed array, [`ClientAccount`]
/// is considerably small. But it would make the code harder
/// to maintain and expand in the future, for little real gain,
/// so I decided against it.
#[derive(Debug, Default)]
pub struct ClientBook {
    clients: IndexMap<ClientId, ClientAccount>,
}

impl ClientBook {
    /// Reads a CSV file from the given path and processes all transactions.
    pub fn from_csv<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let mut reader = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_path(&path)?;

        let mut book = ClientBook::default();

        for result in reader.deserialize() {
            let tx: Transaction = result?;

            if let Err(e) = book.append_tx(tx) {
                eprintln!(
                    "failed to process transaction {:?} for client {:?}: {e}",
                    tx.id, tx.client_id
                );
            }
        }

        Ok(book)
    }

    /// Appends one transaction to the log and updates the related client's
    /// account. If this is a new client, create one.
    pub fn append_tx(&mut self, tx: Transaction) -> Result<(), TransactionError> {
        let client = self
            .clients
            .entry(tx.client_id)
            .or_insert_with(|| ClientAccount::new(tx.client_id));

        client.append_tx(tx)
    }

    pub fn into_clients(self) -> IndexMap<ClientId, ClientAccount> {
        self.clients
    }
}
