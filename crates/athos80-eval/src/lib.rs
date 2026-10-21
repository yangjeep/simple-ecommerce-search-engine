//! Experimental, exact-first commerce pipeline. This is not an Athos API or
//! lexical-parity implementation. All ranking precedes grouping and pagination.
mod engine;
mod schema;
pub use engine::*;
pub use schema::*;

#[cfg(feature = "bm25")]
mod bm25;
