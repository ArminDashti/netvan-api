use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryRange {
    Today,
    Yesterday,
    Week,
    Months,
    All,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
}

impl TimeRange {
    pub fn from_history(range: HistoryRange, custom_start: Option<i64>, custom_end: Option<i64>) -> Self {
        let now = Local::now();
        let today = now.date_naive();
        match range {
            HistoryRange::All => Self {
                start_ts: None,
                end_ts: None,
            },
            HistoryRange::Custom => Self {
                start_ts: custom_start,
                end_ts: custom_end,
            },
            HistoryRange::Today => {
                let start = local_day_start(today);
                Self {
                    start_ts: Some(start),
                    end_ts: Some(Utc::now().timestamp()),
                }
            }
            HistoryRange::Yesterday => {
                let y = today - Duration::days(1);
                Self {
                    start_ts: Some(local_day_start(y)),
                    end_ts: Some(local_day_start(today) - 1),
                }
            }
            HistoryRange::Week => {
                let start_day = today - Duration::days(7);
                Self {
                    start_ts: Some(local_day_start(start_day)),
                    end_ts: Some(Utc::now().timestamp()),
                }
            }
            HistoryRange::Months => {
                let start_day = today - Duration::days(30);
                Self {
                    start_ts: Some(local_day_start(start_day)),
                    end_ts: Some(Utc::now().timestamp()),
                }
            }
        }
    }
}

fn local_day_start(day: NaiveDate) -> i64 {
    let local = Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap_or_else(|| Local::now());
    DateTime::<Utc>::from(local).timestamp()
}
