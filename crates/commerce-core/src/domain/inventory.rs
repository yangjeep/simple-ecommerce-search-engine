#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Availability {
    InStock,
    OutOfStock,
    Backorder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Inventory {
    pub available_units: u32,
    pub status: Availability,
}

impl Inventory {
    pub const fn in_stock(available_units: u32) -> Self {
        Inventory {
            available_units,
            status: Availability::InStock,
        }
    }

    pub const fn out_of_stock() -> Self {
        Inventory {
            available_units: 0,
            status: Availability::OutOfStock,
        }
    }
}
