use std::ops::Neg;

use indexmap::IndexMap;
use rust_decimal::Decimal;
use serde::ser::SerializeStruct;

use crate::transaction::{ClientId, Transaction, TransactionId, TransactionType};

/// A transaction error.
///
/// Given the (short) length of this enum, it might not make sense to even
/// consider it. But I've found myself needing structured errors in the past
/// and once you need it, it's life saving. When expanding the code,
/// this comes in handy.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransactionError {
    #[error("account is locked")]
    LockedAccount,
    #[error("not enough balance to withdraw")]
    NotEnoughBalance,
    #[error("duplicate transaction ids")]
    DuplicateTransactionId,
}

/// A client account.
#[derive(Debug)]
pub struct ClientAccount {
    id: ClientId,

    /// The account's transaction log.
    ///
    /// The Tx IDs are not guaranteed to be ordered, we don't know how
    /// the system generates them. But insertion order is chronological,
    /// thus the use of a IndexMap.
    log: IndexMap<TransactionId, Transaction>,
    /// The list of _active_ disputes.
    ///
    /// Understanding what disputes came and went is as easy as replaying
    /// the transactions, and because this information is not accessed frequently,
    /// it didn't make sense for me to store them after resolution.
    ///
    /// **NOTE:** Because I expect the list to be short, the performance difference
    /// of `Vec` and `HashSet` will be negligible, and for the common case,
    /// I expect `Vec` to be ever so slightly faster.
    disputes: Vec<TransactionId>,

    available: Decimal,
    held: Decimal,
    locked: bool,
}

impl ClientAccount {
    pub fn new(id: ClientId) -> Self {
        Self {
            id,
            // Feels like more than enough for this app.
            log: IndexMap::with_capacity(100),
            // Realistically (unless you are a merchant)
            // how many disputes would a given client have
            // active at any given time? Assuming 10 is enough
            // for most cases.
            disputes: Vec::with_capacity(10),
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            locked: false,
        }
    }

    /// Appends a new transaction to the account's log and calculates
    /// the new account state.
    ///
    /// **NOTE:** This is the only function allowed to alter the state of the log
    /// and its immediate access values, `available`, `held` and `locked`.
    pub fn append_tx(&mut self, tx: Transaction) -> Result<(), TransactionError> {
        if self.locked {
            return Err(TransactionError::LockedAccount);
        }

        let diff = TxDiff::calculate(self, &tx)?;

        self.available += diff.available;
        self.held += diff.held;

        if let Some(lock) = diff.lock {
            self.locked = lock;
        }

        match diff.dispute {
            Some(DisputeAction::Start(id)) => self.disputes.push(id),
            Some(DisputeAction::End(id)) => self.disputes.retain(|dispute| *dispute != id),
            None => {
                if self.log.contains_key(&tx.id) {
                    return Err(TransactionError::DuplicateTransactionId);
                }

                let _ = self.log.insert(tx.id, tx);
            }
        }

        Ok(())
    }

    fn in_dispute(&self, tx: &TransactionId) -> bool {
        self.disputes.contains(tx)
    }

    fn has_balance(&self, amount: Decimal) -> bool {
        self.available >= amount
    }

    // **NOTE:** Though I don't enjoy having OOP-style code (getters/setters) in Rust,
    // some cases benefit from it. This is one of them. The client account
    // contains sensitive information that must not be altered regardless
    // of the ownership of the ClientAccount value.
    //
    // The resulting values for `available`, `held` and `locked` are a result
    // of computing the log of transactions, and no code shall be allowed
    // to temper with them.

    pub fn id(&self) -> ClientId {
        self.id
    }

    pub fn available(&self) -> Decimal {
        self.available
    }

    pub fn held(&self) -> Decimal {
        self.held
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    /// The total funds the client owns, a sum of `available` and `held`.
    pub fn total(&self) -> Decimal {
        self.available + self.held
    }
}

impl serde::Serialize for ClientAccount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // **NOTE:** Implementing Serialize here is mostly unnecessary for
        // this project given `csv`s `write_record` function,
        // but as I did for [`TransactionError`], once you need
        // to serialize it to something else (e.g. JSON),
        // this is how you could expand it.
        //
        // Notice "total", as it's computed on demand.

        let mut ser = serializer.serialize_struct("ClientAccount", 5)?;
        ser.serialize_field("client", &self.id())?;
        ser.serialize_field("available", &self.available())?;
        ser.serialize_field("held", &self.held())?;
        ser.serialize_field("total", &self.total())?;
        ser.serialize_field("locked", &self.locked())?;
        ser.end()
    }
}

/// A transaction's resulting effect.
///
/// All transaction behaviors and its effects are isolated to
/// [`TxDiff::calculate`], which makes it easier to fix or expand
/// behavior logic.
///
/// Decimal values can be either positive or negative, and applying
/// them to the account's state means adding the difference to the
/// current account value.
///
/// **NOTE:** Although this feels, and might be, overkill, I like
/// code with isolated responsibilities, and a diffing system makes
/// it easier to inspect the transaction's effect in a single place.
#[derive(Debug, Default, PartialEq, Eq)]
struct TxDiff {
    available: Decimal,
    held: Decimal,
    /// Present when an account must be locked or freed.
    lock: Option<bool>,
    /// Present when a dispute starts or ends.
    dispute: Option<DisputeAction>,
}

#[derive(Debug, PartialEq, Eq)]
enum DisputeAction {
    Start(TransactionId),
    End(TransactionId),
}

impl TxDiff {
    /// Given a transaction and the client associated to it, calculate
    /// a state difference to be applied.
    ///
    /// This function owns all transaction behaviors and rules.
    fn calculate(client: &ClientAccount, tx: &Transaction) -> Result<Self, TransactionError> {
        match tx.ty {
            TransactionType::Deposit { amount } => return Ok(Self::deposit(amount)),

            TransactionType::Withdrawal { amount } => {
                if !client.has_balance(amount) {
                    return Err(TransactionError::NotEnoughBalance);
                }

                return Ok(Self::withdraw(amount));
            }

            TransactionType::Dispute => {
                if let Some(target) = client.log.get(&tx.id)
                    && let Some(amount) = target.deposit_amount()
                    && !client.in_dispute(&tx.id)
                {
                    return Ok(Self::dispute(tx.id, amount));
                }
            }

            TransactionType::Resolve => {
                if let Some(target) = client.log.get(&tx.id)
                    && let Some(amount) = target.deposit_amount()
                    && client.in_dispute(&tx.id)
                {
                    return Ok(Self::resolve(tx.id, amount));
                }
            }

            TransactionType::Chargeback => {
                if let Some(target) = client.log.get(&tx.id)
                    && let Some(amount) = target.deposit_amount()
                    && client.in_dispute(&tx.id)
                {
                    return Ok(Self::chargeback(tx.id, amount));
                }
            }
        }

        // In any other case, we ignore it.
        Ok(Default::default())
    }

    /// Increases available balance.
    fn deposit(amount: Decimal) -> TxDiff {
        Self {
            available: amount,
            ..Default::default()
        }
    }

    /// Decreases available balance.
    fn withdraw(amount: Decimal) -> TxDiff {
        Self {
            available: amount.neg(),
            ..Default::default()
        }
    }

    /// Holds the disputed amount, decreasing available balance.
    fn dispute(tx: TransactionId, amount: Decimal) -> TxDiff {
        Self {
            available: amount.neg(),
            held: amount,
            dispute: Some(DisputeAction::Start(tx)),
            ..Default::default()
        }
    }

    /// Frees a previously held amount, increasing available balance.
    fn resolve(tx: TransactionId, amount: Decimal) -> TxDiff {
        Self {
            available: amount,
            held: amount.neg(),
            dispute: Some(DisputeAction::End(tx)),
            ..Default::default()
        }
    }

    /// Burns a previously held amount, locking an account.
    fn chargeback(tx: TransactionId, amount: Decimal) -> TxDiff {
        Self {
            held: amount.neg(),
            lock: Some(true),
            dispute: Some(DisputeAction::End(tx)),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    //! **NOTE:** Most of the following tests focus around the [`TxDiff`]
    //! interface as that is the main engine for the transactions.
    //!
    //! I considered using proptest here, but I'm not convinced it would
    //! help enough for the time I have/the test cases present here, to
    //! make it worth implementing. Other parts of the system, particularly
    //! CSV parsing and the [`crate::ClientBook`] pipeline could use it better.

    use rust_decimal::dec;

    use super::*;

    fn client(tys: &[TransactionType]) -> ClientAccount {
        let mut client = ClientAccount::new(ClientId::new(0));
        for ty in tys {
            client
                .append_tx(tx(&client, *ty))
                .expect("valid transactions");
        }
        client
    }

    fn tx(client: &ClientAccount, ty: TransactionType) -> Transaction {
        Transaction {
            ty,
            client_id: client.id(),
            id: TransactionId::new(client.log.len() as u32),
        }
    }

    fn deposit(client: &mut ClientAccount, amount: Decimal) -> TransactionId {
        client
            .append_tx(tx(client, TransactionType::Deposit { amount }))
            .expect("deposit must never fail unless account is locked");
        *client.log.last().unwrap().0
    }

    #[test]
    fn deposit_diff_only_alters_available() {
        let client = client(&[]);
        let amount = dec!(10.0);
        let tx = tx(&client, TransactionType::Deposit { amount });

        let diff = TxDiff::calculate(&client, &tx).expect("deposit diff never fails");
        let expected = TxDiff {
            available: amount,
            ..Default::default()
        };

        assert_eq!(diff, expected);
    }

    #[test]
    fn withdrawal_checks_free_balance() {
        let id = ClientId::new(0);
        let mut client = ClientAccount::new(id);

        let amount = dec!(10.0);
        let tx = tx(&client, TransactionType::Withdrawal { amount });

        let err = TxDiff::calculate(&client, &tx)
            .expect_err("withdrawal fails if not enough balance is available");
        assert_eq!(err, TransactionError::NotEnoughBalance);

        deposit(&mut client, amount);

        let diff = TxDiff::calculate(&client, &tx)
            .expect("withdrawal must succeed if balance is available");

        let expected = TxDiff {
            available: amount.neg(),
            ..Default::default()
        };

        assert_eq!(diff, expected);
    }

    const DISPUTE_RELATED_VARIANTS: [TransactionType; 3] = [
        TransactionType::Dispute,
        TransactionType::Resolve,
        TransactionType::Chargeback,
    ];

    #[test]
    fn dispute_related_is_ignored_for_unknown_tx() {
        let client = client(&[]);

        for ty in DISPUTE_RELATED_VARIANTS {
            let dispute = tx(&client, ty);
            let diff = TxDiff::calculate(&client, &dispute).expect("dispute is valid");
            assert_eq!(diff, TxDiff::default(), "{ty:?} refers to unknown tx");
        }
    }

    #[test]
    fn dispute_related_is_ignored_for_unsupported_tx() {
        let amount = dec!(10.0);
        let client = client(&[
            TransactionType::Deposit { amount },
            TransactionType::Withdrawal { amount },
        ]);

        for ty in DISPUTE_RELATED_VARIANTS {
            let mut dispute = tx(&client, ty);
            dispute.id = *client.log.last().unwrap().0;

            let diff = TxDiff::calculate(&client, &dispute).expect("dispute is valid");
            assert_eq!(diff, TxDiff::default(), "{ty:?} refers to unsupported tx");
        }
    }

    mod dispute {
        use super::*;

        #[test]
        fn is_ignored_for_already_disputed_txs() {
            let amount = dec!(10.0);
            let mut client = client(&[]);

            let deposit_id = deposit(&mut client, amount);
            client.disputes.push(deposit_id);

            let mut dispute = tx(&client, TransactionType::Dispute);
            dispute.id = deposit_id;

            let diff = TxDiff::calculate(&client, &dispute).expect("dispute is valid");
            assert_eq!(
                diff,
                TxDiff::default(),
                "dispute refers to already disputed tx"
            );
        }

        #[test]
        fn holds_disputed_balance() {
            let amount = dec!(10.0);
            let client = client(&[TransactionType::Deposit { amount }]);

            let mut dispute = tx(&client, TransactionType::Dispute);
            dispute.id = *client.log.last().unwrap().0;

            let diff = TxDiff::calculate(&client, &dispute).expect("dispute is valid");
            let expected = TxDiff {
                available: amount.neg(),
                held: amount,
                dispute: Some(DisputeAction::Start(dispute.id)),
                ..Default::default()
            };

            assert_eq!(diff, expected, "dispute not holding balance");
        }
    }

    mod resolve {
        use super::*;

        #[test]
        fn is_ignored_for_undisputed_txs() {
            let amount = dec!(10.0);
            let mut client = client(&[]);
            let deposit_id = deposit(&mut client, amount);

            let mut resolve = tx(&client, TransactionType::Resolve);
            resolve.id = deposit_id;

            let diff = TxDiff::calculate(&client, &resolve).expect("resolve is valid");
            assert_eq!(diff, TxDiff::default(), "resolve refers to undisputed tx");
        }

        #[test]
        fn frees_disputed_balance() {
            let amount = dec!(10.0);
            let mut client = client(&[]);

            let deposit_id = deposit(&mut client, amount);
            client.disputes.push(deposit_id);

            let mut resolve = tx(&client, TransactionType::Resolve);
            resolve.id = deposit_id;

            let diff = TxDiff::calculate(&client, &resolve).expect("resolve is valid");
            let expected = TxDiff {
                available: amount,
                held: amount.neg(),
                dispute: Some(DisputeAction::End(resolve.id)),
                ..Default::default()
            };

            assert_eq!(diff, expected, "resolve not freeing balance");
        }
    }

    mod chargeback {
        use super::*;

        #[test]
        fn is_ignored_for_undisputed_txs() {
            let amount = dec!(10.0);
            let mut client = client(&[]);
            let deposit_id = deposit(&mut client, amount);

            let mut chargeback = tx(&client, TransactionType::Chargeback);
            chargeback.id = deposit_id;

            let diff = TxDiff::calculate(&client, &chargeback).expect("chargeback is valid");
            assert_eq!(
                diff,
                TxDiff::default(),
                "chargeback refers to undisputed tx"
            );
        }

        #[test]
        fn burns_disputed_balance() {
            let amount = dec!(10.0);
            let mut client = client(&[]);

            let deposit_id = deposit(&mut client, amount);
            client.disputes.push(deposit_id);

            let mut chargeback = tx(&client, TransactionType::Chargeback);
            chargeback.id = deposit_id;

            let diff = TxDiff::calculate(&client, &chargeback).expect("chargeback is valid");
            let expected = TxDiff {
                held: amount.neg(),
                lock: Some(true),
                dispute: Some(DisputeAction::End(chargeback.id)),
                ..Default::default()
            };

            assert_eq!(diff, expected, "resolve not burning balance");
        }
    }

    #[test]
    fn append_fails_for_locked_accounts() {
        let mut client = client(&[]);
        client.locked = true;

        let err = client
            .append_tx(tx(&client, TransactionType::Deposit { amount: dec!(10) }))
            .expect_err("account is locked");
        assert_eq!(err, TransactionError::LockedAccount);
    }

    #[test]
    fn append_fails_for_duplicate_tx_ids() {
        let mut client = client(&[]);
        let deposit_id = deposit(&mut client, dec!(10));

        let mut tx = tx(&client, TransactionType::Deposit { amount: dec!(10) });
        tx.id = deposit_id;

        let err = client.append_tx(tx).expect_err("tx id is a duplicate");
        assert_eq!(err, TransactionError::DuplicateTransactionId);
    }

    #[test]
    fn append_updated_client_state_after_multiple_disputes() {
        let mut client = client(&[]);

        client
            .append_tx(tx(&client, TransactionType::Deposit { amount: dec!(10) }))
            .unwrap();
        assert_eq!(client.available, dec!(10));
        assert!(client.held.is_zero());
        assert_eq!(client.total(), dec!(10));
        assert!(!client.locked);
        assert_eq!(client.log.len(), 1);

        client
            .append_tx(tx(&client, TransactionType::Withdrawal { amount: dec!(4) }))
            .unwrap();
        assert_eq!(client.available, dec!(6));
        assert!(client.held.is_zero());
        assert_eq!(client.total(), dec!(6));
        assert!(!client.locked);
        assert_eq!(client.log.len(), 2);

        let mut dispute = tx(&client, TransactionType::Dispute);
        dispute.id = *client.log.first().unwrap().0;
        client.append_tx(dispute).unwrap();
        assert_eq!(client.available, dec!(-4));
        assert_eq!(client.held, dec!(10));
        assert_eq!(client.total(), dec!(6));
        assert!(!client.locked);
        assert_eq!(client.disputes, [dispute.id]);

        let mut resolve = tx(&client, TransactionType::Resolve);
        resolve.id = *client.log.first().unwrap().0;
        client.append_tx(resolve).unwrap();
        assert_eq!(client.available, dec!(6));
        assert!(client.held.is_zero());
        assert_eq!(client.total(), dec!(6));
        assert!(!client.locked);
        assert!(client.disputes.is_empty());

        let mut dispute = tx(&client, TransactionType::Dispute);
        dispute.id = *client.log.first().unwrap().0;
        client.append_tx(dispute).unwrap();
        let mut chargeback = tx(&client, TransactionType::Chargeback);
        chargeback.id = *client.log.first().unwrap().0;
        client.append_tx(chargeback).unwrap();
        assert_eq!(client.available, dec!(-4));
        assert!(client.held.is_zero());
        assert_eq!(client.total(), dec!(-4));
        assert!(client.locked);
        assert!(client.disputes.is_empty());
    }
}
