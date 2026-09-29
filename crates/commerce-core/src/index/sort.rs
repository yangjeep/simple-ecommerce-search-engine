//! Issue #79 (E3b): physical sort execution for single numeric fields.
//!
//! E3 (#77) found native 71x slower than the fastest competitor on a
//! filter + numeric sort PLP cell because no sort structure existed: every
//! matching variant's full attribute map was cloned to read one value, then
//! the whole candidate set was fully sorted to keep 48. This module adds the
//! two preregistered alternatives -- deliberately *not* a general expression
//! sort engine:
//!
//! - **Strategy A** ([`top_k_scan`]): a dense ordinal-indexed `f64` column
//!   ([`NumericSortColumn`]) read for each candidate, with a bounded
//!   `limit`-sized heap. Cost `O(|C| log limit)`, independent of `N`.
//! - **Strategy B** ([`top_k_presorted`]): walk the value-sorted
//!   `(value, ordinal)` list the index *already* keeps for range filters
//!   ([`CatalogIndex::numeric_sorted`](super::CatalogIndex::numeric_sorted)),
//!   membership-test each ordinal against the candidate bitmap, stop after
//!   `limit` hits. Cost is roughly `limit * N / |C|` entries inspected, so it
//!   wins when the candidate set is dense and loses badly when it is narrow.
//!
//! Both implement exactly one ordering, [`SortKey`]:
//! present values before missing ones (in **both** directions), then value
//! ascending/descending, then variant ordinal ascending (catalog encounter
//! order) as the deterministic tie-break. `NaN` is treated as missing.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use roaring::RoaringBitmap;

use super::CatalogIndex;

/// Sort direction for one numeric field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

/// One sorted hit: the variant ordinal plus its sort value (`None` =
/// missing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SortedHit {
    pub ordinal: u32,
    pub value: Option<f64>,
}

/// Result of a bounded sort: the first `limit` hits in [`SortKey`] order and
/// how many ordinals the strategy inspected to produce them.
#[derive(Debug, Clone, PartialEq)]
pub struct SortOutcome {
    pub hits: Vec<SortedHit>,
    pub inspected: u64,
}

/// The total order every strategy (and the E3b oracle) implements. Smaller
/// is better, so the first `limit` keys in ascending `Ord` are the answer.
#[derive(Debug, Clone, Copy)]
pub struct SortKey {
    missing: bool,
    /// The value, negated for descending order so "better" is always
    /// "smaller"; ignored when `missing`.
    directed: f64,
    ordinal: u32,
    /// The original value (`None` if missing/NaN); carried for output, never
    /// compared.
    value: Option<f64>,
}

impl SortKey {
    #[must_use]
    pub fn new(value: Option<f64>, ordinal: u32, direction: Direction) -> Self {
        match value {
            Some(v) if !v.is_nan() => SortKey {
                missing: false,
                directed: match direction {
                    Direction::Ascending => v,
                    Direction::Descending => -v,
                },
                ordinal,
                value: Some(v),
            },
            _ => SortKey {
                missing: true,
                directed: 0.0,
                ordinal,
                value: None,
            },
        }
    }

    #[must_use]
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }

    #[must_use]
    pub fn value(&self) -> Option<f64> {
        self.value
    }
}

impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SortKey {}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.missing
            .cmp(&other.missing)
            .then_with(|| {
                if self.missing {
                    Ordering::Equal
                } else {
                    // Normalize -0.0 to 0.0 so a descending sort's negation
                    // cannot split equal zero values into two tie groups.
                    (self.directed + 0.0).total_cmp(&(other.directed + 0.0))
                }
            })
            .then_with(|| self.ordinal.cmp(&other.ordinal))
    }
}

/// Strategy A's physical structure: a dense `Vec<f64>` indexed by variant
/// ordinal, `NaN` as the explicit missing-value sentinel. `8 * N` bytes per
/// field -- a new structure this index did not previously keep.
#[derive(Debug, Clone)]
pub struct NumericSortColumn {
    values: Vec<f64>,
}

impl NumericSortColumn {
    /// Builds the column for `attribute` from the index's existing
    /// value-sorted list. `None` if the attribute has no numeric values.
    #[must_use]
    pub fn build(index: &CatalogIndex, attribute: &str) -> Option<Self> {
        let sorted = index.numeric_sorted(attribute)?;
        let mut values = vec![f64::NAN; index.ordinal_count()];
        for &(value, ordinal) in sorted {
            values[ordinal as usize] = value;
        }
        Some(NumericSortColumn { values })
    }

    #[must_use]
    pub fn value(&self, ordinal: u32) -> Option<f64> {
        self.values
            .get(ordinal as usize)
            .copied()
            .filter(|v| !v.is_nan())
    }

    /// Owned heap bytes (on-heap estimate: the value buffer only).
    #[must_use]
    pub fn owned_bytes(&self) -> usize {
        self.values.capacity() * std::mem::size_of::<f64>()
    }
}

/// Strategy B's only new structure: which ordinals *have* a (non-NaN) value
/// for the field, so the missing-values tail can be produced as
/// `candidates - presence` in ordinal order. The ordered list itself is the
/// index's pre-existing `numeric_sorted` range-filter structure.
#[derive(Debug, Clone)]
pub struct PresenceBitmap {
    present: RoaringBitmap,
}

impl PresenceBitmap {
    #[must_use]
    pub fn build(index: &CatalogIndex, attribute: &str) -> Option<Self> {
        let sorted = index.numeric_sorted(attribute)?;
        let mut ordinals: Vec<u32> = sorted
            .iter()
            .filter(|(value, _)| !value.is_nan())
            .map(|&(_, ordinal)| ordinal)
            .collect();
        ordinals.sort_unstable();
        let present = RoaringBitmap::from_sorted_iter(ordinals)
            .expect("ordinals are sorted and a variant carries one value per numeric field");
        Some(PresenceBitmap { present })
    }

    #[must_use]
    pub fn contains(&self, ordinal: u32) -> bool {
        self.present.contains(ordinal)
    }

    /// Serialized roaring size (on-heap estimate, same convention as
    /// `CatalogIndex::approximate_size_bytes`).
    #[must_use]
    pub fn owned_bytes(&self) -> usize {
        self.present.serialized_size()
    }
}

/// Strategy A: read every candidate's value from `column` and keep the best
/// `limit` in a bounded max-heap (top = current worst kept). Never sorts
/// more than `limit` elements. `inspected == |candidates|`.
#[must_use]
pub fn top_k_scan(
    candidates: &RoaringBitmap,
    column: &NumericSortColumn,
    direction: Direction,
    limit: usize,
) -> SortOutcome {
    if limit == 0 {
        return SortOutcome {
            hits: Vec::new(),
            inspected: 0,
        };
    }
    let mut heap: BinaryHeap<SortKey> = BinaryHeap::with_capacity(limit + 1);
    let mut inspected = 0u64;
    for ordinal in candidates {
        inspected += 1;
        let key = SortKey::new(column.value(ordinal), ordinal, direction);
        if heap.len() < limit {
            heap.push(key);
        } else if heap.peek().is_some_and(|worst| key < *worst) {
            heap.pop();
            heap.push(key);
        }
    }
    let kept = heap.into_sorted_vec();
    SortOutcome {
        hits: kept
            .into_iter()
            .map(|key| SortedHit {
                ordinal: key.ordinal,
                value: key.value,
            })
            .collect(),
        inspected,
    }
}

/// Strategy B: walk `sorted` (the index's value-ascending list, see
/// `CatalogIndex::numeric_sorted`) in `direction`, keeping ordinals that are
/// in `candidates`, and stop after `limit` hits.
///
/// Ties must come out in ascending ordinal order in both directions. The
/// list is ordered by `f64::total_cmp` from a stable sort of
/// ordinal-ordered input, so each run of `total_cmp`-equal values is already
/// ordinal-ascending -- except that `total_cmp` puts `-0.0` before `0.0`
/// while the preregistered order treats them as equal. So the walk goes by
/// runs of *numerically* equal values (`==`), and inside a run merges its
/// (at most two, sign-split) ordinal-ascending sub-runs by ordinal. NaN
/// entries (at the extremes of a `total_cmp` order) are skipped as missing.
/// If fewer than `limit` candidates have a value, the missing-value tail is
/// `candidates - presence` in ordinal order. `inspected` counts every list
/// entry and every tail candidate examined.
#[must_use]
pub fn top_k_presorted(
    sorted: &[(f64, u32)],
    presence: &PresenceBitmap,
    candidates: &RoaringBitmap,
    direction: Direction,
    limit: usize,
) -> SortOutcome {
    presorted_walk(
        sorted,
        presence,
        |ordinal| candidates.contains(ordinal),
        candidates.iter(),
        direction,
        limit,
    )
}

/// Issue #63 (P2): [`top_k_presorted`] for the match-all candidate set
/// `0..count` without materializing it: every list entry is a member, so
/// the membership test is skipped, and the missing-value tail walks
/// `0..count`. Identical output to `top_k_presorted` over the full bitmap.
#[must_use]
pub fn top_k_presorted_all(
    sorted: &[(f64, u32)],
    presence: &PresenceBitmap,
    count: u32,
    direction: Direction,
    limit: usize,
) -> SortOutcome {
    presorted_walk(sorted, presence, |_| true, 0..count, direction, limit)
}

/// The walk shared by [`top_k_presorted`] and [`top_k_presorted_all`]:
/// `member` decides candidate membership for list entries, and `tail`
/// yields the candidate set in ascending ordinal order for the
/// missing-value tail.
fn presorted_walk<M, T>(
    sorted: &[(f64, u32)],
    presence: &PresenceBitmap,
    member: M,
    tail: T,
    direction: Direction,
    limit: usize,
) -> SortOutcome
where
    M: Fn(u32) -> bool,
    T: IntoIterator<Item = u32>,
{
    let mut hits = Vec::with_capacity(limit);
    let mut inspected = 0u64;
    if limit == 0 {
        return SortOutcome { hits, inspected };
    }
    // Drop NaN entries: negative NaNs sort first, positive NaNs last.
    let lo = sorted.partition_point(|(v, _)| v.is_nan() && v.is_sign_negative());
    let hi = lo + sorted[lo..].partition_point(|(v, _)| !v.is_nan());
    let valid = &sorted[lo..hi];
    inspected += (sorted.len() - valid.len()) as u64;

    let mut run_start;
    let mut run_end;
    let mut cursor = match direction {
        Direction::Ascending => 0,
        Direction::Descending => valid.len(),
    };
    let mut done = false;
    loop {
        match direction {
            Direction::Ascending => {
                if cursor >= valid.len() {
                    break;
                }
                let value = valid[cursor].0;
                run_start = cursor;
                run_end = cursor + valid[cursor..].partition_point(|(v, _)| *v <= value);
                cursor = run_end;
            }
            Direction::Descending => {
                if cursor == 0 {
                    break;
                }
                let value = valid[cursor - 1].0;
                run_end = cursor;
                run_start = valid[..cursor].partition_point(|(v, _)| *v < value);
                cursor = run_start;
            }
        }
        if merge_run(
            &valid[run_start..run_end],
            &member,
            limit,
            &mut hits,
            &mut inspected,
        ) {
            done = true;
            break;
        }
    }
    if !done {
        for ordinal in tail {
            if presence.contains(ordinal) {
                continue;
            }
            inspected += 1;
            hits.push(SortedHit {
                ordinal,
                value: None,
            });
            if hits.len() >= limit {
                break;
            }
        }
    }
    SortOutcome { hits, inspected }
}

/// Emits the candidates of one run of numerically-equal values in ordinal
/// order. A run is ordinal-ascending except when it holds both `-0.0` and
/// `0.0` entries, which `total_cmp` splits into two ordinal-ascending
/// sub-runs (negative first); those are merged. Returns `true` once `hits`
/// reaches `limit`.
fn merge_run<M: Fn(u32) -> bool>(
    run: &[(f64, u32)],
    member: &M,
    limit: usize,
    hits: &mut Vec<SortedHit>,
    inspected: &mut u64,
) -> bool {
    let split = run.partition_point(|(v, _)| v.is_sign_negative());
    let (mut a, mut b) = (
        run[..split].iter().peekable(),
        run[split..].iter().peekable(),
    );
    loop {
        let next = match (a.peek(), b.peek()) {
            (Some(x), Some(y)) => {
                if x.1 <= y.1 {
                    a.next()
                } else {
                    b.next()
                }
            }
            (Some(_), None) => a.next(),
            (None, Some(_)) => b.next(),
            (None, None) => return false,
        };
        let &(value, ordinal) = next.expect("peeked");
        *inspected += 1;
        if member(ordinal) {
            hits.push(SortedHit {
                ordinal,
                value: Some(value),
            });
            if hits.len() >= limit {
                return true;
            }
        }
    }
}

/// Facet counting path for one facet field (Issue #79 F1/F2/F3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetPath {
    /// Strategy A: dense ordinal column scan over the candidate set.
    OrdinalScan,
    /// Strategy B: per-value bitmap `intersection_len`.
    BitmapCount,
}

/// Issue #79 F3's preregistered deterministic rule: bitmap counting iff
/// `|C_f| >= tau * V_f`, where `V_f` is the facet's dictionary size. An
/// attribute the ordinal column cannot answer exactly always uses bitmap
/// counting regardless of `tau`.
#[must_use]
pub fn choose_facet_path(
    candidate_count: u64,
    cardinality: usize,
    tau: f64,
    ordinal_exact: bool,
) -> FacetPath {
    if !ordinal_exact || candidate_count as f64 >= tau * cardinality as f64 {
        FacetPath::BitmapCount
    } else {
        FacetPath::OrdinalScan
    }
}

/// Sort execution path for one sorted request (Issue #79 S1/S2/S3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortPath {
    /// Strategy A: candidate scan + bounded heap.
    CandidateTopK,
    /// Strategy B: precomputed order + membership test.
    Presorted,
}

/// Issue #79 S3's preregistered deterministic rule: presorted iff
/// `|C|^2 >= rho * limit * N` -- strategy B inspects about `limit * N / |C|`
/// entries against strategy A's `|C|`.
#[must_use]
pub fn choose_sort_path(
    candidate_count: u64,
    limit: usize,
    ordinal_count: usize,
    rho: f64,
) -> SortPath {
    let c = candidate_count as f64;
    if c * c >= rho * limit as f64 * ordinal_count as f64 {
        SortPath::Presorted
    } else {
        SortPath::CandidateTopK
    }
}
