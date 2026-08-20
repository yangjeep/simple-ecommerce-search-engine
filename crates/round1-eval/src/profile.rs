//! Real-data catalog profiler (Area 1): measures the messiness a
//! deterministic ingestion pipeline actually encounters, rather than
//! assuming it away. Every number here is `evidence class: real`.

use std::collections::{HashMap, HashSet};

use crate::data::RealProduct;

#[derive(Debug, Clone)]
pub struct CatalogProfileReport {
    pub product_count: usize,
    pub missing_description: usize,
    pub missing_bullets: usize,
    pub missing_brand: usize,
    pub missing_color: usize,
    pub distinct_raw_brand_strings: usize,
    pub distinct_normalized_brands: usize,
    pub brand_normalization_collisions: usize,
    pub distinct_raw_color_strings: usize,
    pub distinct_normalized_colors: usize,
    pub color_normalization_collisions: usize,
    pub duplicate_title_groups: usize,
    pub products_in_a_duplicate_title_group: usize,
    pub title_word_count_p50: usize,
    pub title_word_count_p95: usize,
    pub top_brands_by_count: Vec<(String, usize)>,
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

fn percentile(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

pub fn profile(products: &[RealProduct]) -> CatalogProfileReport {
    let mut raw_brands: HashSet<&str> = HashSet::new();
    let mut normalized_brands: HashSet<String> = HashSet::new();
    let mut raw_colors: HashSet<&str> = HashSet::new();
    let mut normalized_colors: HashSet<String> = HashSet::new();
    let mut brand_counts: HashMap<String, usize> = HashMap::new();
    let mut title_counts: HashMap<&str, usize> = HashMap::new();
    let mut title_word_counts: Vec<usize> = Vec::with_capacity(products.len());

    let mut missing_description = 0;
    let mut missing_bullets = 0;
    let mut missing_brand = 0;
    let mut missing_color = 0;

    for p in products {
        if p.description.is_none() {
            missing_description += 1;
        }
        if p.bullets.is_none() {
            missing_bullets += 1;
        }
        if let Some(brand) = &p.brand {
            raw_brands.insert(brand.as_str());
            let key = normalize(brand);
            normalized_brands.insert(key.clone());
            *brand_counts.entry(key).or_insert(0) += 1;
        } else {
            missing_brand += 1;
        }
        if let Some(color) = &p.color {
            raw_colors.insert(color.as_str());
            normalized_colors.insert(normalize(color));
        } else {
            missing_color += 1;
        }
        *title_counts.entry(p.title.as_str()).or_insert(0) += 1;
        title_word_counts.push(p.title.split_whitespace().count());
    }

    let duplicate_groups: Vec<usize> = title_counts.values().copied().filter(|&c| c > 1).collect();
    let duplicate_title_groups = duplicate_groups.len();
    let products_in_a_duplicate_title_group = duplicate_groups.iter().sum();

    title_word_counts.sort_unstable();

    let mut top_brands: Vec<(String, usize)> = brand_counts.into_iter().collect();
    top_brands.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top_brands.truncate(15);

    CatalogProfileReport {
        product_count: products.len(),
        missing_description,
        missing_bullets,
        missing_brand,
        missing_color,
        distinct_raw_brand_strings: raw_brands.len(),
        distinct_normalized_brands: normalized_brands.len(),
        brand_normalization_collisions: raw_brands.len().saturating_sub(normalized_brands.len()),
        distinct_raw_color_strings: raw_colors.len(),
        distinct_normalized_colors: normalized_colors.len(),
        color_normalization_collisions: raw_colors.len().saturating_sub(normalized_colors.len()),
        duplicate_title_groups,
        products_in_a_duplicate_title_group,
        title_word_count_p50: percentile(&title_word_counts, 0.5),
        title_word_count_p95: percentile(&title_word_counts, 0.95),
        top_brands_by_count: top_brands,
    }
}

impl CatalogProfileReport {
    pub fn print(&self) {
        let pct = |n: usize| n as f64 / self.product_count as f64 * 100.0;
        println!("=== Real catalog profile (evidence class: real) ===");
        println!("product_count: {}", self.product_count);
        println!(
            "missing_description: {} ({:.1}%)",
            self.missing_description,
            pct(self.missing_description)
        );
        println!(
            "missing_bullets: {} ({:.1}%)",
            self.missing_bullets,
            pct(self.missing_bullets)
        );
        println!(
            "missing_brand: {} ({:.1}%)",
            self.missing_brand,
            pct(self.missing_brand)
        );
        println!(
            "missing_color: {} ({:.1}%)",
            self.missing_color,
            pct(self.missing_color)
        );
        println!(
            "distinct_raw_brand_strings: {}  distinct_normalized_brands: {}  collisions_removed_by_normalization: {}",
            self.distinct_raw_brand_strings, self.distinct_normalized_brands, self.brand_normalization_collisions
        );
        println!(
            "distinct_raw_color_strings: {}  distinct_normalized_colors: {}  collisions_removed_by_normalization: {}",
            self.distinct_raw_color_strings, self.distinct_normalized_colors, self.color_normalization_collisions
        );
        println!(
            "duplicate_title_groups: {}  products_in_a_duplicate_title_group: {} ({:.1}%)",
            self.duplicate_title_groups,
            self.products_in_a_duplicate_title_group,
            pct(self.products_in_a_duplicate_title_group)
        );
        println!(
            "title_word_count: p50={} p95={}",
            self.title_word_count_p50, self.title_word_count_p95
        );
        println!("top 15 brands by product count:");
        for (brand, count) in &self.top_brands_by_count {
            println!("  {brand:<30} {count}");
        }
        println!();
    }
}
