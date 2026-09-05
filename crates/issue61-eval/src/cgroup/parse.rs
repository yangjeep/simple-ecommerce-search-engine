use super::CgroupError;
use std::collections::HashMap;

pub(super) struct CpuCounters {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
}

impl CpuCounters {
    pub fn parse(content: &str) -> Result<Self, CgroupError> {
        let values = parse_flat_counters(content)?;
        Ok(Self {
            usage_usec: required(&values, "usage_usec")?,
            user_usec: required(&values, "user_usec")?,
            system_usec: required(&values, "system_usec")?,
            nr_periods: required(&values, "nr_periods")?,
            nr_throttled: required(&values, "nr_throttled")?,
            throttled_usec: required(&values, "throttled_usec")?,
        })
    }
}

pub(super) fn parse_flat_counters(content: &str) -> Result<HashMap<String, u64>, CgroupError> {
    content
        .lines()
        .map(|line| {
            let mut fields = line.split_whitespace();
            let key = fields
                .next()
                .ok_or(CgroupError::MissingField("counter key"))?;
            let raw = fields
                .next()
                .ok_or(CgroupError::MissingField("counter value"))?;
            Ok((key.to_owned(), parse_integer(key, raw)?))
        })
        .collect()
}

pub(super) fn parse_pressure(content: &str) -> Result<(u64, u64), CgroupError> {
    let mut some = None;
    let mut full = None;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let category = fields.next();
        let total = fields.find_map(|field| field.strip_prefix("total="));
        match (category, total) {
            (Some("some"), Some(raw)) => {
                some = Some(parse_integer("cpu.pressure some total", raw)?)
            }
            (Some("full"), Some(raw)) => {
                full = Some(parse_integer("cpu.pressure full total", raw)?)
            }
            _ => {}
        }
    }
    Ok((
        some.ok_or(CgroupError::MissingField("cpu.pressure some total"))?,
        full.ok_or(CgroupError::MissingField("cpu.pressure full total"))?,
    ))
}

pub(super) fn required(
    values: &HashMap<String, u64>,
    field: &'static str,
) -> Result<u64, CgroupError> {
    values
        .get(field)
        .copied()
        .ok_or(CgroupError::MissingField(field))
}

pub(super) fn parse_integer(field: &str, value: &str) -> Result<u64, CgroupError> {
    value
        .parse::<u64>()
        .map_err(|_| CgroupError::InvalidInteger {
            field: field.to_owned(),
            value: value.to_owned(),
        })
}
