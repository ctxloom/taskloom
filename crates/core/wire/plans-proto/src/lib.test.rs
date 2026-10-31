//! The wire types carry the names the kernel routes by, and round-trip.

use super::v1;
use prost::{Message, Name};

// The kernel routes and upcasts by type URL, so each message must carry its
// own full name with angzarr's "/" prefix.
#[test]
fn test_type_url_names_each_message_by_full_name() {
    assert_eq!(v1::PlanAdded::type_url(), "/ctxloom.plans.v1.PlanAdded");
    assert_eq!(v1::AddPlan::type_url(), "/ctxloom.plans.v1.AddPlan");
    assert_eq!(v1::PlanState::type_url(), "/ctxloom.plans.v1.PlanState");
}

#[test]
fn test_event_round_trips_through_its_encoding() {
    let event = v1::MarkedSuperseded {
        by: "brisk-cedar-otter".to_string(),
        from_status: v1::PlanStatus::Active as i32,
    };
    let decoded = v1::MarkedSuperseded::decode(event.encode_to_vec().as_slice()).expect("decode");
    assert_eq!(decoded, event);
}
