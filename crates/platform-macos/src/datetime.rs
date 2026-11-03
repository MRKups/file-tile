//! Localized macOS date and time formatting following active OS region and clock settings.
use std::cell::RefCell;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const MIN_UNIX_SECS: f64 = -62_135_596_800.0;
const MAX_UNIX_SECS: f64 = 253_402_300_799.0;

thread_local! {
    static FORMATTER: RefCell<Option<(Instant, Retained<NSDateFormatter>)>> =
        const { RefCell::new(None) };
}

pub fn format_local_datetime(time: SystemTime) -> Option<String> {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs_f64(),
        Err(error) => -error.duration().as_secs_f64(),
    };
    if !seconds.is_finite() || !(MIN_UNIX_SECS..=MAX_UNIX_SECS).contains(&seconds) {
        return None;
    }
    let date = NSDate::dateWithTimeIntervalSince1970(seconds);
    let raw = FORMATTER.with(|slot| {
        let now = Instant::now();
        let mut guard = slot.borrow_mut();
        let expired = guard
            .as_ref()
            .is_none_or(|(created, _)| now.saturating_duration_since(*created) >= REFRESH_INTERVAL);
        if expired {
            let formatter = NSDateFormatter::new();
            formatter.setDateStyle(NSDateFormatterStyle::ShortStyle);
            formatter.setTimeStyle(NSDateFormatterStyle::ShortStyle);
            *guard = Some((now, formatter));
        }
        guard
            .as_ref()
            .map(|(_, formatter)| formatter.stringFromDate(&date).to_string())
    })?;
    if raw.is_empty() {
        return None;
    }
    Some(normalize_spaces(&raw))
}

fn normalize_spaces(text: &str) -> String {
    if text.contains(['\u{202F}', '\u{00A0}']) {
        text.chars()
            .map(|ch| {
                if matches!(ch, '\u{202F}' | '\u{00A0}') {
                    ' '
                } else {
                    ch
                }
            })
            .collect()
    } else {
        text.to_owned()
    }
}
