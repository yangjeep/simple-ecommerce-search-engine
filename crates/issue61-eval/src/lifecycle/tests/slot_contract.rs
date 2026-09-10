use super::{run1, FakePort};
use crate::lifecycle::EventType;
use crate::{CampaignSeries, Dataset, WorkloadProjection};
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn every_started_slot_has_one_result_and_complete_teardown() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("lifecycle succeeds");

    // Then
    let mut events_by_slot = BTreeMap::<String, Vec<String>>::new();
    for line in fake.event_lines() {
        let event: Value = serde_json::from_str(line).expect("event evidence is JSON");
        if event["slot_index"].is_null() {
            continue;
        }
        let key = format!(
            "{}:{}:{}:{}:{}:{}:{}:{}",
            event["phase"],
            event["series"],
            event["dataset"],
            event["projection"],
            event["block_index"],
            event["attempt"],
            event["slot_index"],
            event["engine"]
        );
        events_by_slot.entry(key).or_default().push(
            event["event_type"]
                .as_str()
                .expect("event type is a string")
                .to_owned(),
        );
    }
    for events in events_by_slot.values() {
        assert_eq!(count(events, EventType::SlotStarted), 1);
        assert_eq!(
            count(events, EventType::SlotCompleted) + count(events, EventType::SlotFailed),
            1
        );
        assert_eq!(count(events, EventType::TeardownStarted), 1);
        assert_eq!(
            count(events, EventType::TeardownCompleted) + count(events, EventType::TeardownFailed),
            1
        );
    }
}

#[test]
fn warm_rejection_limits_are_isolated_by_exact_campaign_series() {
    // Given
    let mut fake = FakePort::default();
    let all = CampaignSeries::Warm {
        dataset: Dataset::Wands,
        projection: WorkloadProjection::All,
    };
    let fast_path = CampaignSeries::Warm {
        dataset: Dataset::Wands,
        projection: WorkloadProjection::FastPath,
    };
    for block in 0..6 {
        fake.reject_before_accept(all, block, 1);
    }
    fake.reject_before_accept(fast_path, 0, 1);

    // When
    run1(&mut fake).expect("separate warm cells remain below their own limits");

    // Then
    assert_eq!(fake.exact_rejection_count(all), 6);
    assert_eq!(fake.exact_rejection_count(fast_path), 1);
}

fn count(events: &[String], event_type: EventType) -> usize {
    let expected = serde_json::to_value(event_type).expect("event type serializes");
    events
        .iter()
        .filter(|event| Value::String((*event).clone()) == expected)
        .count()
}
