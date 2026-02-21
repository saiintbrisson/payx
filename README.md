# PayX

My take at a payment engine implementation in Rust. Given a set of transactions, declared in a CSV, process each sequentially and compute the final state of each client account.

Run it with:

```sh
cargo run -- data/sample/in.csv > out.csv
# or
just test
```

## Behavior

1. Transactions are records with a unique TxID, a unique client ID, the transaction type and an associated amount, present when the type requires so (deposits and withdrawals).
2. Clients are composed of its ID, a log of transactions related to it, the available and held balances, as in a double-entry bookkeeping system. Its total funds are a sum of both values.

Each transaction is appended to a client's transaction log, and a difference is computed given the transaction type:
* deposits and withdrawals are simple additions/deductions from the available balance,
* disputes refer to previous transactions and freeze the amount depositted in the original Tx,
* resolutions happen on top of disputes and signal that the amount is now free to use again,
* finally, chargebacks signal the end of a dispute, essentially burning that amount (likely returning it to a requesting partner), and locks the account.

## Design

I think of the client account as nothing but the result of a series of transactions. Still, a system requires frequent access to certain fields, such as its available and held amounts, if it's locked, the total funds, and so on, which here I'll call _snapshots_. So we have to have them stored somewhere, as up-to-date as possible. We wouldn't want to replay the entire log every time we want to access one of these values.

For this reason, the [client accounts](./src/client.rs) are mutated only by a single function, `append_tx`, which calculates a difference using `TxDiff`, and simply applies by adding the results to its _snapshot_ fields. I believe this is a strong way to keep track of places that modify the snapshot, and avoid future developers from doing unwanted updates to those very important values.

Through testing the `TxDiff::calculate` function, I can check for all effects that certain operations cause, without having to "reverse" what happened from the final balances.

### Code

Some relevant points come to mind:

1. First, the code is sprinkled with `**NOTE:**`, which give relevant context to each portion of the codebase. I suggest reading them!
2. My code is not overly documented. I believe certain fields and functions do not require docstrings, just like I believe code does not have to be commented if its behavior is obvious in most cases. As such, functions like `ClientAccount::id()` or the `Transaction::client_id` don't have docstrings.
3. From the start, I chose to have client IDs and TxIDs be newtypes, sealed in their own modules to avoid anyone tempering with their inner fields.
4. The `ClientBook` is not that far off from what an async implementation would do. Each client book can act as an actor, and you write through MPSC channels.
5. There were some viable performance optimizations for the case described, particularly around u16 client IDs, like removing the Client ID->Account map entirely in favor of a O(1) read using boxed arrays, given accounts are of a reasonably small size. I ultimately decided against. The solution is more cumbersome than a simple map for little gain in most cases, and in a real world environment, unless you either have 65k clients, or a translation layer of real IDs->0..65K mapped IDs (like sharding the payment engine), it wouldn't work.
6. It is possible to dispute deposits regardless of whether the account has enough available balance to cover the original amount. This is obvious, but important to point out. Available can become negative, and a user would have to deposit enough to cover this deficit before being able to transfer funds again.

