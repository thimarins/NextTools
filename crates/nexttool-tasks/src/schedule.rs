//! Schedule recurrence in LOCAL time (v1: runs only while the app is open).

use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveTime, TimeZone, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Recurrence {
    /// Once at a Unix timestamp.
    Once { at: i64 },
    /// Every day at hh:mm local time.
    Daily { hour: u32, minute: u32 },
    /// On the given weekdays (0 = Monday … 6 = Sunday) at hh:mm local time.
    Weekly { days: Vec<u8>, hour: u32, minute: u32 },
}

#[derive(Debug)]
pub enum RecurrenceError {
    Invalid(String),
}

impl std::error::Error for RecurrenceError {}

impl std::fmt::Display for RecurrenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecurrenceError::Invalid(s) => write!(f, "{s}"),
        }
    }
}

impl Recurrence {
    pub fn validate(&self, now: i64) -> Result<(), RecurrenceError> {
        match self {
            Recurrence::Once { at } if *at <= now => Err(RecurrenceError::Invalid("a data/hora precisa estar no futuro".into())),
            Recurrence::Daily { hour, minute } | Recurrence::Weekly { hour, minute, .. } if *hour > 23 || *minute > 59 => {
                Err(RecurrenceError::Invalid("horário inválido".into()))
            }
            Recurrence::Weekly { days, .. } if days.is_empty() || days.iter().any(|d| *d > 6) => {
                Err(RecurrenceError::Invalid("escolha ao menos um dia da semana válido".into()))
            }
            _ => Ok(()),
        }
    }

    /// Next run strictly after `after` (Unix seconds), or None when finished.
    pub fn next_after(&self, after: i64) -> Option<i64> {
        self.next_after_tz(after, &Local)
    }

    pub fn next_after_tz<Tz: TimeZone>(&self, after: i64, tz: &Tz) -> Option<i64> {
        match self {
            Recurrence::Once { at } => (*at > after).then_some(*at),
            Recurrence::Daily { hour, minute } => next_matching(after, tz, *hour, *minute, |_| true),
            Recurrence::Weekly { days, hour, minute } => next_matching(after, tz, *hour, *minute, |wd| days.contains(&(wd.num_days_from_monday() as u8))),
        }
    }

    pub fn describe(&self) -> String {
        const DN: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
        match self {
            Recurrence::Once { at } => format!("uma vez em {}", Local.timestamp_opt(*at, 0).single().map(|d| d.format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default()),
            Recurrence::Daily { hour, minute } => format!("diariamente às {hour:02}:{minute:02}"),
            Recurrence::Weekly { days, hour, minute } => {
                let mut d = days.clone();
                d.sort();
                format!("{} às {hour:02}:{minute:02}", d.iter().map(|x| DN[*x as usize]).collect::<Vec<_>>().join(", "))
            }
        }
    }
}

fn next_matching<Tz: TimeZone>(after: i64, tz: &Tz, hour: u32, minute: u32, day_ok: impl Fn(Weekday) -> bool) -> Option<i64> {
    let start: DateTime<Tz> = tz.timestamp_opt(after, 0).single()?;
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    let mut date: NaiveDate = start.date_naive();
    for _ in 0..8 {
        if day_ok(date.weekday()) {
            let local = date.and_time(time);
            // DST gaps: skip non-existent local times; ambiguous → earliest.
            let ts = match tz.from_local_datetime(&local) {
                LocalResult::Single(t) => Some(t.timestamp()),
                LocalResult::Ambiguous(a, _) => Some(a.timestamp()),
                LocalResult::None => None,
            };
            if let Some(ts) = ts {
                if ts > after {
                    return Some(ts);
                }
            }
        }
        date += Duration::days(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn brt() -> FixedOffset {
        FixedOffset::west_opt(3 * 3600).unwrap() // UTC-3
    }

    fn ts(y: i32, m: u32, d: u32, h: u32, mi: u32) -> i64 {
        brt().with_ymd_and_hms(y, m, d, h, mi, 0).unwrap().timestamp()
    }

    #[test]
    fn daily_weekly_once() {
        let tz = brt();
        // Wednesday 2026-11-18 10:00 local.
        let now = ts(2026, 11, 18, 10, 0);
        let daily = Recurrence::Daily { hour: 22, minute: 30 };
        assert_eq!(daily.next_after_tz(now, &tz), Some(ts(2026, 11, 18, 22, 30)));
        assert_eq!(daily.next_after_tz(ts(2026, 11, 18, 22, 30), &tz), Some(ts(2026, 11, 19, 22, 30)), "strictly after");
        let weekly = Recurrence::Weekly { days: vec![0, 4], hour: 2, minute: 0 }; // Mon, Fri
        assert_eq!(weekly.next_after_tz(now, &tz), Some(ts(2026, 11, 20, 2, 0)), "next Friday");
        assert_eq!(weekly.next_after_tz(ts(2026, 11, 20, 2, 0), &tz), Some(ts(2026, 11, 23, 2, 0)), "then Monday");
        let once = Recurrence::Once { at: now + 60 };
        assert_eq!(once.next_after_tz(now, &tz), Some(now + 60));
        assert_eq!(once.next_after_tz(now + 60, &tz), None, "finished after running");
    }

    #[test]
    fn validation() {
        let now = 1_000_000;
        assert!(Recurrence::Once { at: now - 1 }.validate(now).is_err());
        assert!(Recurrence::Daily { hour: 24, minute: 0 }.validate(now).is_err());
        assert!(Recurrence::Weekly { days: vec![], hour: 1, minute: 0 }.validate(now).is_err());
        assert!(Recurrence::Weekly { days: vec![7], hour: 1, minute: 0 }.validate(now).is_err());
        assert!(Recurrence::Weekly { days: vec![2], hour: 1, minute: 0 }.validate(now).is_ok());
        assert_eq!(Recurrence::Weekly { days: vec![4, 0], hour: 2, minute: 5 }.describe(), "seg, sex às 02:05");
    }
}
