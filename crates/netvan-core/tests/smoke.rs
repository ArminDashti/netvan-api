use netvan_core::db::Database;
use netvan_core::history::{HistoryRange, TimeRange};
use netvan_core::settings::AppSettings;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn history_ranges_produce_bounds() {
    let today = TimeRange::from_history(HistoryRange::Today, None, None);
    assert!(today.start_ts.is_some());
    let all = TimeRange::from_history(HistoryRange::All, None, None);
    assert!(all.start_ts.is_none());
}

#[test]
fn sqlite_roundtrip_settings() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("netvan-test-{nanos}.db"));
    let db = Database::open(&path).unwrap();
    let mut s = AppSettings::default();
    s.ping_interval_secs = 9;
    db.save_settings(&s).unwrap();
    let loaded = db.get_settings().unwrap();
    assert_eq!(loaded.ping_interval_secs, 9);
    let _ = std::fs::remove_file(path);
}
