//! The calendar, against Go's spelling of instants and its dates.

use super::{date, rfc3339_nano};
use ports::Timestamp;

#[test]
fn test_instants_are_written_as_gos_rfc3339_nano() {
    let cases = [
        (-62_135_596_800, 0, "0001-01-01T00:00:00Z"),
        (951_868_799, 5, "2000-02-29T23:59:59.000000005Z"),
        (-86_400, 100_000_000, "1969-12-31T00:00:00.1Z"),
        (0, 0, "1970-01-01T00:00:00Z"),
        (-2_203_977_600, 0, "1900-02-28T00:00:00Z"),
        (-2_203_891_200, 0, "1900-03-01T00:00:00Z"),
        (4_107_542_400, 0, "2100-03-01T00:00:00Z"),
        (1_709_164_800, 0, "2024-02-29T00:00:00Z"),
        (1_677_628_800, 0, "2023-03-01T00:00:00Z"),
        (1_704_067_199, 999_999_999, "2023-12-31T23:59:59.999999999Z"),
        (1_775_046_645, 123_456_000, "2026-04-01T12:30:45.123456Z"),
        (951_782_400, 0, "2000-02-29T00:00:00Z"),
        (951_868_800, 0, "2000-03-01T00:00:00Z"),
        (978_307_199, 0, "2000-12-31T23:59:59Z"),
    ];
    for (seconds, nanos, go) in cases {
        assert_eq!(rfc3339_nano(Timestamp { seconds, nanos }), go, "{seconds}");
    }
}

// A date is its midnight, UTC; one that is no date, or not spelled
// `YYYY-MM-DD`, is none.
#[test]
fn test_a_date_is_its_midnight_and_no_date_is_none() {
    let midnight = |seconds| Some(Timestamp { seconds, nanos: 0 });
    assert_eq!(date("1970-01-01"), midnight(0));
    assert_eq!(date("2000-02-29"), midnight(951_782_400));
    assert_eq!(date("2024-02-29"), midnight(1_709_164_800));
    assert_eq!(date("1900-03-01"), midnight(-2_203_891_200));
    for bad in [
        "2023-02-29",
        "1900-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-04-31",
        "2026-04-00",
        "2026-4-01",
        "2026/04/01",
        "2026-04-0x",
        "+026-04-01",
    ] {
        assert_eq!(date(bad), None, "{bad}");
    }
}
