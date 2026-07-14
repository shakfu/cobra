//! Money.
//!
//! Accounting is done in whole credits as `i64`. Integers (not floats) so that
//! balances are exact and reproducible; pricing may compute in `f64` but is
//! rounded to credits at the boundary. `i64` credits give headroom well beyond
//! any plausible galactic economy while staying trivially comparable for
//! byte-identical determinism dumps.

/// A quantity of money, in whole credits.
pub type Money = i64;

/// A quantity of goods, in whole units.
pub type Quantity = u32;
