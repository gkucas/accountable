# accountable

A toy payments engine for a Rust coding challenge.

It reads transactions from a CSV, applies them to client accounts, and writes
final account balances to stdout.

## Running

```sh
cargo run -- transactions.csv > accounts.csv
```

## High-Level Design

I treated this exercise as a small transactional system rather than just a CSV
transformation.

The code is split into a few focused modules:

- `reader`: parses CSV rows into domain transactions
- `model/transaction`: transaction identities, actions, and typed operation
  state
- `model/client`: client account behavior
- `model/ledger`: ledger orchestration and transaction routing

## Design Choices

### Parallelism Without Locks

One design goal was to demonstrate concurrency without shared mutable state
protected by locks.

I chose to shard work across multiple ledgers based on `client_id`. Each ledger
owns a disjoint subset of clients and processes its own incoming channel. This
means:

- all transactions for the same client are routed to the same worker
- workers do not need to coordinate through mutexes
- ownership stays local to each ledger task

This is more elaborate than required for the challenge, but I wanted to show one
way to structure parallel processing with message passing instead of lock-based
synchronization.

In a production system a messaging broker could be used that would implement
similar behaviour.

### Domain Model as Entities

I modeled the main concepts explicitly as domain entities instead with strict
types and invariants enforcing business rules in the type system.

This was intentional even though this is a toy application. The types are meant
to make the business rules easier to follow:

- `Ledger` decides where a transaction should be processed
- `Client` applies balance changes
- `TransactionAction` distinguishes user-visible actions such as deposit,
  withdrawal, dispute, resolve, and chargeback

I do not mean this model to imply this is the exact storage or runtime design I
would use in a real financial system. It is closer to an in-memory domain model
for the exercise.

### Double-Booking-Inspired Thinking

I wanted the balances to behave like a ledger rather than as a few ad hoc
counters.

The guiding idea was:

- balances should be explainable by the operations that produced them
- debits and credits should have clear algebraic meaning
- account state should be testable through invariants

This led me toward a double-booking-inspired approach where balances can be
reasoned about as the sum of operations rather than only as independently
mutated numeric fields.

That idea also motivated the property tests asserting that account balances are
consistent with the stored transaction sets.

### Storno and Typed Transaction States

Another design goal was to explore whether disputes and reversals could be
represented through derived transactions rather than bespoke imperative logic.

I used:

- `Pending` and `Applied` typed transaction states
- `storno` to express the reversal of an already-applied operation
- deterministic child transaction IDs derived from parent transaction IDs

The intended benefit was to make reversal-like flows composable and
type-directed. In principle, that would let the system treat deposits,
withdrawals, and follow-up operations more uniformly.

## What Did Not Work as Well

The storno-based dispute model was the most experimental part of the
implementation, and it did not end up as cleanly as I originally intended.

In particular:

- I wanted disputes to be expressible as transformations on operations
- I wanted the model to work uniformly for deposits, withdrawals, and even
  follow-up actions

## Correctness and Testing

For simplicity I used `rust_decimal` instead of floating point arithmetic to
avoid precision issues in financial values. Alternatively an unsigned integer
value could be used with a fixed decimal place offset.

I also added tests around:

- parser behavior
- dispute, resolve, and chargeback flows
- deterministic child transaction ID derivation
- invariant-style properties for stored transactions and balances

The property tests were especially useful for checking that balances stay
consistent with the stored debit/credit history.

## Safety and Robustness

- CSV input is streamed rather than loaded all at once
- invalid reference operations are handled as errors rather than causing panics
- fixed-point decimal arithmetic is used for money
- work is partitioned by client ownership to avoid shared mutable state across
  workers

## Efficiency

The implementation streams CSV rows and processes them incrementally.

That said, disputes require historical transaction lookup, so transaction data
is retained in memory. This is acceptable for the challenge but would need a
more deliberate storage strategy in a real system.

The multi-ledger sharding is mainly here to demonstrate lock-free concurrency
through ownership and routing. Whether I would keep that exact design in
production would depend on actual workload shape and operational constraints.

## Assumptions

- clients are created on first deposit or withdrawal
- dispute-like actions for nonexistent clients are rejected
- client routing is stable because all actions for a client hash to the same
  ledger shard
