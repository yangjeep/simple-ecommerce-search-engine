use std::fmt;

macro_rules! typed_id {
    ($name:ident, $repr:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub $repr);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<$repr> for $name {
            fn from(value: $repr) -> Self {
                $name(value)
            }
        }
    };
}

typed_id!(ProductId, u64);
typed_id!(VariantId, u64);
typed_id!(BrandId, u32);
typed_id!(CategoryId, u32);
typed_id!(ProductTypeId, u32);
