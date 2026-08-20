#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Currency {
    Usd,
}

/// Money is always integer minor units (cents) to keep comparisons and
/// range filters exact; no floating point currency math anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price {
    pub amount_cents: i64,
    pub currency: Currency,
}

impl Price {
    pub const fn usd(amount_cents: i64) -> Self {
        Price {
            amount_cents,
            currency: Currency::Usd,
        }
    }
}
