use commerce_core::index::tokenize;

pub(crate) fn tokenize_residuals(residual: &[String]) -> Vec<String> {
    residual.iter().flat_map(|term| tokenize(term)).collect()
}

#[cfg(test)]
mod tests {
    use super::tokenize_residuals;

    #[test]
    fn residual_tokens_when_compiler_preserves_multiword_phrase_are_flattened() {
        // Given
        let residual = vec![
            "47".to_string(),
            "inch".to_string(),
            "tv wall mount".to_string(),
        ];

        // When
        let actual = tokenize_residuals(&residual);

        // Then
        assert_eq!(actual, ["47", "inch", "tv", "wall", "mount"]);
    }
}
