//! Deterministic, analytical bytes-touched estimates (amendment 1, section
//! 3 "Common rules"). No hardware counters exist on this host, so these are
//! computed from the actual structures: roaring container sizes (an array
//! container stores 2 bytes per element up to 4,096 elements, a bitmap
//! container is 8 KiB) and distinct 64-byte cache lines of a dense column.
//! They are labelled estimates in every report.

use std::collections::BTreeMap;

use roaring::RoaringBitmap;

pub const CACHE_LINE: u64 = 64;
const ARRAY_LIMIT: u64 = 4096;
const BITMAP_CONTAINER_BYTES: u64 = 8192;

/// Cardinality of each 65,536-ordinal chunk (one roaring container) present
/// in `bitmap`, keyed by the chunk's high 16 bits.
#[must_use]
pub fn chunk_cardinalities(bitmap: &RoaringBitmap) -> BTreeMap<u32, u64> {
    let mut chunks = BTreeMap::new();
    for ordinal in bitmap {
        *chunks.entry(ordinal >> 16).or_insert(0) += 1;
    }
    chunks
}

/// Bytes of one roaring container holding `cardinality` elements.
#[must_use]
pub const fn container_bytes(cardinality: u64) -> u64 {
    if cardinality > ARRAY_LIMIT {
        BITMAP_CONTAINER_BYTES
    } else {
        2 * cardinality
    }
}

/// Container bytes read by one full pass over `bitmap`.
#[must_use]
pub fn bitmap_bytes(bitmap: &RoaringBitmap) -> u64 {
    chunk_cardinalities(bitmap)
        .values()
        .map(|&c| container_bytes(c))
        .sum()
}

/// Distinct cache lines of a dense column with `element_bytes`-byte
/// elements touched by gathering every ordinal of `bitmap`, in bytes.
#[must_use]
pub fn column_gather_bytes(bitmap: &RoaringBitmap, element_bytes: u64) -> u64 {
    let mut lines = 0u64;
    let mut last: Option<u64> = None;
    for ordinal in bitmap {
        let line = u64::from(ordinal) * element_bytes / CACHE_LINE;
        if last != Some(line) {
            lines += 1;
            last = Some(line);
        }
    }
    lines * CACHE_LINE
}

/// Bytes a value-bitmap facet pass reads: every value bitmap's containers,
/// plus, for each chunk a value bitmap shares with the candidate set, that
/// candidate container.
#[must_use]
pub fn bitmap_facet_bytes<'a>(
    candidates: &RoaringBitmap,
    value_bitmaps: impl IntoIterator<Item = &'a RoaringBitmap>,
) -> u64 {
    let candidate_chunks = chunk_cardinalities(candidates);
    let mut total = 0u64;
    for value in value_bitmaps {
        for (key, cardinality) in chunk_cardinalities(value) {
            total += container_bytes(cardinality);
            if let Some(&c) = candidate_chunks.get(&key) {
                total += container_bytes(c);
            }
        }
    }
    total
}

/// Bytes written while materializing `0..count` by per-element insertion
/// (P0): each chunk passes through an array container up to 4,096 elements,
/// then is rewritten as an 8 KiB bitmap container.
#[must_use]
pub fn per_element_materialization_bytes(count: u32) -> u64 {
    let mut total = 0u64;
    let mut remaining = u64::from(count);
    while remaining > 0 {
        let chunk = remaining.min(65_536);
        total += if chunk > ARRAY_LIMIT {
            2 * ARRAY_LIMIT + BITMAP_CONTAINER_BYTES
        } else {
            2 * chunk
        };
        remaining -= chunk;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_and_line_arithmetic() {
        let dense: RoaringBitmap = (0..70_000).collect();
        // One full bitmap container plus a 4,464-element container, which is
        // also above the array limit.
        assert_eq!(bitmap_bytes(&dense), 2 * 8192);
        let sparse: RoaringBitmap = [1u32, 2, 70_000].into_iter().collect();
        assert_eq!(bitmap_bytes(&sparse), 2 * 2 + 2);
        // u32 column: 16 ordinals per 64-byte line.
        let run: RoaringBitmap = (0..32).collect();
        assert_eq!(column_gather_bytes(&run, 4), 2 * 64);
        let strided: RoaringBitmap = (0..4).map(|i| i * 16).collect();
        assert_eq!(column_gather_bytes(&strided, 4), 4 * 64);
        assert_eq!(per_element_materialization_bytes(0), 0);
        assert_eq!(per_element_materialization_bytes(10), 20);
        assert_eq!(per_element_materialization_bytes(65_536 + 10), 16_384 + 20);
    }

    #[test]
    fn bitmap_facet_bytes_counts_shared_candidate_containers() {
        let candidates: RoaringBitmap = (0..10).collect();
        let a: RoaringBitmap = [1u32, 2].into_iter().collect();
        let b: RoaringBitmap = [70_000u32].into_iter().collect();
        // a: 4 bytes + shared candidate chunk 20 bytes; b: 2 bytes, no overlap.
        assert_eq!(bitmap_facet_bytes(&candidates, [&a, &b]), 4 + 20 + 2);
    }
}
