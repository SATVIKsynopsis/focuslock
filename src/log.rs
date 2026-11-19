use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Duration as Days, Local, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    SessionStart {
        minutes: u32,
    },
    SessionEnd {
        planned_minutes: u32,
        focused_minutes: u32,
        how: String,
    },
    BlockedApp {
        bundle_id: String,
    },
    BlockedUrl {
        domain: String,
    },
    PassGranted {
        reason: String,
    },
    QuitWithReason {
        reason: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Line {
    ts: DateTime<Local>,
    #[serde(flatten)]
    event: Event,
}

pub fn log_path() -> PathBuf {
    crate::config::data_dir().join("events.jsonl")
}

pub fn record(event: Event) {
    let line = Line { ts: Local::now(), event };
    let result = (|| -> std::io::Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(log_path())?;
        let json = serde_json::to_string(&line).map_err(std::io::Error::other)?;
        writeln!(f, "{json}")
    })();
    if let Err(e) = result {
        eprintln!("focuslock: could not write event log: {e}");
    }
}

pub fn error(msg: &str) {
    eprintln!("focuslock: {msg}");
    let path = crate::config::data_dir().join("errors.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {msg}", Local::now().to_rfc3339());
    }
}

#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Totals {
    pub sessions_completed: u32,
    pub focus_minutes: u32,
    pub blocked_attempts: u32,
    pub passes_used: u32,
    pub top_distractions: Vec<Distraction>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Distraction {
    pub id: String,
    pub kind: &'static str,
    pub count: u32,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Day {
    pub date: String,
    pub weekday: String,
    pub focus_minutes: u32,
    pub sessions_completed: u32,
    pub blocked_attempts: u32,
}

#[derive(Debug, Serialize)]
pub struct Recent {
    pub time: String,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub today: Totals,
    pub week: Totals,
    pub days: Vec<Day>,
    pub recent: Vec<Recent>,
}

pub fn report() -> Report {
    let text = fs::read_to_string(log_path()).unwrap_or_default();
    build_report(&text, Local::now().date_naive())
}

fn build_report(jsonl: &str, today: NaiveDate) -> Report {
    let week_start = today - Days::days(6);
    let lines: Vec<Line> = jsonl
        .lines()
        .filter_map(|l| serde_json::from_str::<Line>(l).ok())
        .filter(|l| {
            let d = l.ts.date_naive();
            d >= week_start && d <= today
        })
        .collect();

    let on = |date: NaiveDate| totals(lines.iter().filter(move |l| l.ts.date_naive() == date));

    let days = (0..7)
        .map(|i| {
            let date = week_start + Days::days(i);
            let t = on(date);
            Day {
                date: date.format("%Y-%m-%d").to_string(),
                weekday: date.format("%a").to_string(),
                focus_minutes: t.focus_minutes,
                sessions_completed: t.sessions_completed,
                blocked_attempts: t.blocked_attempts,
            }
        })
        .collect();

    let recent = lines
        .iter()
        .rev()
        .filter(|l| l.ts.date_naive() == today)
        .take(30)
        .map(|l| Recent { time: l.ts.format("%H:%M").to_string(), event: l.event.clone() })
        .collect();

    Report { today: on(today), week: totals(lines.iter()), days, recent }
}

fn totals<'a>(lines: impl Iterator<Item = &'a Line>) -> Totals {
    let mut t = Totals::default();
    let mut counts: HashMap<(String, &'static str), u32> = HashMap::new();
    for line in lines {
        match &line.event {
            Event::SessionEnd { focused_minutes, how, .. } => {
                t.focus_minutes += focused_minutes;
                if how == "completed" {
                    t.sessions_completed += 1;
                }
            }
            Event::BlockedApp { bundle_id } => {
                t.blocked_attempts += 1;
                *counts.entry((bundle_id.clone(), "app")).or_default() += 1;
            }
            Event::BlockedUrl { domain } => {
                t.blocked_attempts += 1;
                *counts.entry((domain.clone(), "site")).or_default() += 1;
            }
            Event::PassGranted { .. } => t.passes_used += 1,
            _ => {}
        }
    }
    let mut top: Vec<_> =
        counts.into_iter().map(|((id, kind), count)| Distraction { id, kind, count }).collect();
    top.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.id.cmp(&b.id)));
    top.truncate(5);
    t.top_distractions = top;
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_format() {
        let line = Line { ts: Local::now(), event: Event::BlockedUrl { domain: "youtube.com".into() } };
        let json = serde_json::to_string(&line).unwrap();
        assert!(json.contains(r#""event":"blocked_url""#), "{json}");
        assert!(json.contains(r#""domain":"youtube.com""#), "{json}");
    }

    #[test]
    fn report() {
        let at = |day: &str, hm: &str| {
            let naive = chrono::NaiveDateTime::parse_from_str(&format!("{day} {hm}"), "%Y-%m-%d %H:%M")
                .unwrap();
            naive.and_local_timezone(Local).unwrap().to_rfc3339()
        };
        let l = |ts: String, rest: &str| format!(r#"{{"ts":"{ts}",{rest}}}"#);
        let log = [
            l(at("2026-11-19", "09:00"), r#""event":"session_start","minutes":25"#),
            l(at("2026-11-19", "09:05"), r#""event":"blocked_app","bundle_id":"com.apple.Music""#),
            l(at("2026-11-19", "09:06"), r#""event":"blocked_url","domain":"youtube.com""#),
            l(at("2026-11-19", "09:07"), r#""event":"blocked_url","domain":"youtube.com""#),
            l(at("2026-11-19", "09:08"), r#""event":"pass_granted","reason":"need the AWS console""#),
            l(
                at("2026-11-19", "09:25"),
                r#""event":"session_end","planned_minutes":25,"focused_minutes":25,"how":"completed""#,
            ),
            l(
                at("2026-11-19", "10:00"),
                r#""event":"session_end","planned_minutes":50,"focused_minutes":10,"how":"stopped""#,
            ),
            "not json at all".to_string(),
            l(
                at("2026-11-17", "09:00"),
                r#""event":"session_end","planned_minutes":50,"focused_minutes":50,"how":"completed""#,
            ),
            l(at("2026-11-17", "09:10"), r#""event":"blocked_url","domain":"youtube.com""#),
            l(at("2026-11-01", "09:00"), r#""event":"blocked_app","bundle_id":"old""#),
        ]
        .join("\n");

        let today = NaiveDate::from_ymd_opt(2026, 11, 19).unwrap();
        let r = build_report(&log, today);

        assert_eq!(r.today.sessions_completed, 1);
        assert_eq!(r.today.focus_minutes, 35);
        assert_eq!(r.today.blocked_attempts, 3);
        assert_eq!(r.today.passes_used, 1);
        assert_eq!(
            r.today.top_distractions[0],
            Distraction { id: "youtube.com".into(), kind: "site", count: 2 }
        );

        assert_eq!(r.week.focus_minutes, 85);
        assert_eq!(r.week.sessions_completed, 2);
        assert_eq!(r.week.top_distractions[0].count, 3);

        assert_eq!(r.days.len(), 7);
        assert_eq!(r.days[0].date, "2026-11-13");
        assert_eq!(r.days[4].focus_minutes, 50);
        assert_eq!(r.days[6].date, "2026-11-19");
        assert_eq!(r.days[6].focus_minutes, 35);

        assert_eq!(r.recent.len(), 7);
        assert_eq!(r.recent[0].time, "10:00");
    }
}
