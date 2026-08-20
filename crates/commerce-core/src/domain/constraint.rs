use super::attribute::{AttributeMap, AttributeValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericOp {
    Eq,
    Lt,
    Lte,
    Gt,
    Gte,
}

impl NumericOp {
    fn apply(self, lhs: f64, rhs: f64) -> bool {
        match self {
            NumericOp::Eq => lhs == rhs,
            NumericOp::Lt => lhs < rhs,
            NumericOp::Lte => lhs <= rhs,
            NumericOp::Gt => lhs > rhs,
            NumericOp::Gte => lhs >= rhs,
        }
    }
}

/// A single typed constraint over one attribute. A query compiles into a
/// list of these (Gate 2); Gate 1 only needs them to prove that evaluating a
/// constraint set against one variant's combined attributes is variant-safe.
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    Enum {
        attribute: String,
        value: String,
    },
    MultiEnumContains {
        attribute: String,
        value: String,
    },
    Boolean {
        attribute: String,
        value: bool,
    },
    Numeric {
        attribute: String,
        op: NumericOp,
        value: f64,
    },
    Text {
        attribute: String,
        contains: String,
    },
}

impl Constraint {
    fn attribute_name(&self) -> &str {
        match self {
            Constraint::Enum { attribute, .. }
            | Constraint::MultiEnumContains { attribute, .. }
            | Constraint::Boolean { attribute, .. }
            | Constraint::Numeric { attribute, .. }
            | Constraint::Text { attribute, .. } => attribute,
        }
    }

    pub fn matches(&self, attributes: &AttributeMap) -> bool {
        let Some(value) = attributes.get(self.attribute_name()) else {
            return false;
        };
        match (self, value) {
            (Constraint::Enum { value: want, .. }, AttributeValue::Enum(have)) => have == want,
            (
                Constraint::MultiEnumContains { value: want, .. },
                AttributeValue::MultiEnum(have),
            ) => have.iter().any(|v| v == want),
            (Constraint::Boolean { value: want, .. }, AttributeValue::Boolean(have)) => {
                have == want
            }
            (
                Constraint::Numeric {
                    op, value: want, ..
                },
                AttributeValue::Numeric(have),
            ) => op.apply(*have, *want),
            (Constraint::Text { contains, .. }, AttributeValue::Text(have)) => {
                have.contains(contains.as_str())
            }
            _ => false,
        }
    }
}

pub fn all_match(constraints: &[Constraint], attributes: &AttributeMap) -> bool {
    constraints.iter().all(|c| c.matches(attributes))
}
