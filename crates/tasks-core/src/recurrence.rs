//! Calendar recurrence and timezone conversion, independent from persistence.
//!
//! Repeating schedules use local calendar dates, never fixed 24-hour durations.
//! UTC is used only for notification instants. Ambiguous times choose the first
//! occurrence; nonexistent times move forward to the first valid local minute.
use chrono::{
    DateTime, Datelike, Days, Duration, LocalResult, Months, NaiveDate, NaiveTime, SecondsFormat,
    TimeZone, Utc,
};
use chrono_tz::Tz;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

use crate::{Result, ServiceError};

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(ServiceError::Invalid(message.into()))
}

fn date(value: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        ServiceError::Invalid("Recurrence dates must use YYYY-MM-DD and be valid dates.".into())
    })?;
    if value.len() != 10
        || parsed.format("%Y-%m-%d").to_string() != value
        || !(1..=9999).contains(&parsed.year())
    {
        return invalid("Recurrence dates must use YYYY-MM-DD.");
    }
    Ok(parsed)
}

fn nullable_date(value: Option<&Value>, key: &str) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            date(value)?;
            Ok(Some(value.clone()))
        }
        _ => invalid(format!("{key} must be a date string or null.")),
    }
}

/// Normalize a recurrence rule without consulting a task or database.
///
/// `count` includes the current occurrence. `weekdays` are ISO weekdays (1–7).
/// `anchor_date` is optional, and preserves month-end/leap-day intent across
/// shortened months. Callers should set it to the first due date for date-based
/// monthly and yearly recurrence; completion-based rules intentionally ignore it.
pub(super) fn validate_rule(value: &Value) -> Result<Value> {
    let rule = value
        .as_object()
        .ok_or_else(|| ServiceError::Invalid("recurrence must be an object or null.".into()))?;
    let allowed = [
        "frequency",
        "interval",
        "weekdays",
        "end_date",
        "count",
        "mode",
        "anchor_date",
        "month_days",
        "ordinal_weekday",
        "workday",
        "year_month",
        "skip_weekends",
    ];
    if let Some(key) = rule.keys().find(|key| !allowed.contains(&key.as_str())) {
        return invalid(format!("Unknown recurrence field: {key}"));
    }
    let frequency = rule
        .get("frequency")
        .and_then(Value::as_str)
        .filter(|frequency| ["daily", "weekly", "monthly", "yearly"].contains(frequency))
        .ok_or_else(|| {
            ServiceError::Invalid(
                "Recurrence frequency must be daily, weekly, monthly, or yearly.".into(),
            )
        })?;
    let interval = match rule.get("interval") {
        None => 1,
        Some(value) => value
            .as_u64()
            .filter(|value| (1..=365).contains(value))
            .ok_or_else(|| {
                ServiceError::Invalid(
                    "Recurrence interval must be an integer from 1 to 365.".into(),
                )
            })?,
    };
    let mode = match rule.get("mode") {
        None => "due_date",
        Some(Value::String(mode)) if ["due_date", "completion_date"].contains(&mode.as_str()) => {
            mode
        }
        _ => return invalid("Recurrence mode must be due_date or completion_date."),
    };
    let mut weekdays = match rule.get("weekdays") {
        None => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .filter(|day| (1..=7).contains(day))
                    .ok_or_else(|| {
                        ServiceError::Invalid(
                            "Recurrence weekdays must be integers from 1 (Monday) to 7 (Sunday)."
                                .into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return invalid("Recurrence weekdays must be an array."),
    };
    weekdays.sort_unstable();
    weekdays.dedup();
    if frequency != "weekly" && !weekdays.is_empty() {
        return invalid("Weekdays can only be selected for a weekly recurrence.");
    }
    let count = match rule.get("count") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .filter(|count| (1..=1_000_000).contains(count))
                .ok_or_else(|| {
                    ServiceError::Invalid(
                        "Recurrence count must be an integer from 1 to 1000000 or null.".into(),
                    )
                })?,
        ),
    };
    let end_date = nullable_date(rule.get("end_date"), "Recurrence end_date")?;
    let anchor_date = nullable_date(rule.get("anchor_date"), "Recurrence anchor_date")?;
    let mut month_days = match rule.get("month_days") {
        None => vec![],
        Some(Value::Array(values)) if values.len() <= 32 => values
            .iter()
            .map(|v| {
                v.as_i64()
                    .filter(|day| *day == -1 || (1..=31).contains(day))
                    .ok_or_else(|| {
                        ServiceError::Invalid(
                            "month_days must contain 1..31 or -1 for the last day.".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return invalid("month_days must be an array of at most 32 dates."),
    };
    month_days.sort_unstable();
    month_days.dedup();
    let ordinal_weekday = match rule.get("ordinal_weekday") {
        None | Some(Value::Null) => None,
        Some(Value::Object(value))
            if value.len() == 2
                && value.contains_key("ordinal")
                && value.contains_key("weekday") =>
        {
            let ordinal = value["ordinal"]
                .as_i64()
                .filter(|v| *v == -1 || (1..=5).contains(v))
                .ok_or_else(|| {
                    ServiceError::Invalid("ordinal must be 1..5 or -1 for last.".into())
                })?;
            let weekday = value["weekday"]
                .as_i64()
                .filter(|v| (1..=7).contains(v))
                .ok_or_else(|| {
                    ServiceError::Invalid("ordinal weekday must be 1 (Monday)..7 (Sunday).".into())
                })?;
            Some(json!({"ordinal":ordinal,"weekday":weekday}))
        }
        _ => return invalid("ordinal_weekday must be {ordinal, weekday} or null."),
    };
    let workday = match rule.get("workday") {
        None | Some(Value::Null) => None,
        Some(Value::String(v)) if matches!(v.as_str(), "first" | "last") => Some(v.as_str()),
        _ => return invalid("workday must be first, last, or null."),
    };
    let year_month =
        match rule.get("year_month") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().filter(|v| (1..=12).contains(v)).ok_or_else(|| {
                ServiceError::Invalid("year_month must be 1..12 or null.".into())
            })?),
        };
    let skip_weekends = match rule.get("skip_weekends") {
        None => false,
        Some(Value::Bool(v)) => *v,
        _ => return invalid("skip_weekends must be a boolean."),
    };
    let patterns = usize::from(!month_days.is_empty())
        + usize::from(ordinal_weekday.is_some())
        + usize::from(workday.is_some());
    if patterns > 1 {
        return invalid("Select only one monthly date, ordinal weekday, or workday pattern.");
    }
    if patterns > 0 && !matches!(frequency, "monthly" | "yearly") {
        return invalid("Month patterns require monthly or yearly recurrence.");
    }
    if workday.is_some() && frequency != "monthly" {
        return invalid("The first/last workday pattern is monthly only.");
    }
    if frequency == "yearly" && (month_days.len() > 1 || month_days.contains(&-1)) {
        return invalid("Yearly recurrence selects one calendar day from 1 to 31.");
    }
    if year_month.is_some() && (frequency != "yearly" || patterns == 0) {
        return invalid("year_month requires a yearly date or ordinal weekday pattern.");
    }
    if skip_weekends && (frequency != "monthly" || ordinal_weekday.is_some() || workday.is_some()) {
        return invalid("Skip weekends applies to monthly calendar dates.");
    }
    if let (Some(month), Some(day)) = (year_month, month_days.first()) {
        // A leap year permits February 29 while rejecting dates that never exist.
        if NaiveDate::from_ymd_opt(2000, month as u32, *day as u32).is_none() {
            return invalid("The selected yearly date does not exist in that month.");
        }
    }
    Ok(json!({
        "frequency": frequency,
        "interval": interval,
        "weekdays": weekdays,
        "mode": mode,
        "end_date": end_date,
        "count": count,
        "anchor_date": anchor_date,
        "month_days": month_days,
        "ordinal_weekday": ordinal_weekday,
        "workday": workday,
        "year_month": year_month,
        "skip_weekends": skip_weekends,
    }))
}

fn out_of_range() -> ServiceError {
    ServiceError::Invalid("The next recurrence falls outside the supported calendar range.".into())
}

fn clamped_day(year: i32, month: u32, desired_day: u32) -> Result<NaiveDate> {
    for day in (1..=desired_day).rev() {
        if let Some(value) = NaiveDate::from_ymd_opt(year, month, day) {
            if (1..=9999).contains(&year) {
                return Ok(value);
            }
            return Err(out_of_range());
        }
    }
    Err(out_of_range())
}

fn month_candidates(
    year: i32,
    month: u32,
    rule: &Value,
    fallback_day: u32,
) -> Result<Vec<NaiveDate>> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(out_of_range)?;
    let last = clamped_day(year, month, 31)?;
    let mut candidates = Vec::new();
    if let Some(pattern) = rule["ordinal_weekday"].as_object() {
        let weekday = pattern["weekday"].as_i64().expect("validated weekday") as u32;
        let ordinal = pattern["ordinal"].as_i64().expect("validated ordinal");
        let day = if ordinal == -1 {
            last.day() - (7 + last.weekday().number_from_monday() - weekday) % 7
        } else {
            1 + (7 + weekday - first.weekday().number_from_monday()) % 7 + (ordinal as u32 - 1) * 7
        };
        if let Some(candidate) = NaiveDate::from_ymd_opt(year, month, day) {
            candidates.push(candidate);
        }
    } else if let Some(workday) = rule["workday"].as_str() {
        let mut candidate = if workday == "first" { first } else { last };
        while candidate.weekday().number_from_monday() > 5 {
            candidate = if workday == "first" {
                candidate.succ_opt()
            } else {
                candidate.pred_opt()
            }
            .ok_or_else(out_of_range)?;
        }
        candidates.push(candidate);
    } else {
        let days = rule["month_days"].as_array().expect("validated month days");
        if days.is_empty() {
            candidates.push(clamped_day(year, month, fallback_day)?);
        } else {
            for value in days {
                let day = value.as_i64().expect("validated month day");
                if day == -1 {
                    candidates.push(last);
                } else if let Some(candidate) = NaiveDate::from_ymd_opt(year, month, day as u32) {
                    candidates.push(candidate);
                }
            }
        }
    }
    if rule["skip_weekends"] == true {
        candidates.retain(|date| date.weekday().number_from_monday() <= 5);
    }
    candidates.sort_unstable();
    candidates.dedup();
    Ok(candidates)
}

fn has_custom_pattern(rule: &Value) -> bool {
    !rule["month_days"]
        .as_array()
        .expect("validated days")
        .is_empty()
        || !rule["ordinal_weekday"].is_null()
        || !rule["workday"].is_null()
        || !rule["year_month"].is_null()
        || rule["skip_weekends"] == true
}

/// Check the pattern independently of exhausted count/end limits. Gregorian
/// calendar structure repeats every400years; only reachable month phases matter.
pub(super) fn validate_calendar(due: &str, rule: &Value) -> Result<()> {
    let rule = validate_rule(rule)?;
    if !has_custom_pattern(&rule) || rule["mode"] == "completion_date" {
        return Ok(());
    }
    let due = date(due)?;
    let anchor = rule["anchor_date"]
        .as_str()
        .map(date)
        .transpose()?
        .unwrap_or(due);
    let yearly = rule["frequency"] == "yearly";
    let month = if yearly {
        rule["year_month"]
            .as_u64()
            .map(|m| m as u32)
            .unwrap_or(anchor.month())
    } else {
        due.month()
    };
    let step =
        rule["interval"].as_u64().expect("validated interval") as u32 * if yearly { 12 } else { 1 };
    let mut phase = due.year().rem_euclid(400) as u32 * 12 + month - 1;
    let mut visited = HashSet::new();
    while visited.insert(phase) {
        if !month_candidates(
            2000 + (phase / 12) as i32,
            phase % 12 + 1,
            &rule,
            anchor.day(),
        )?
        .is_empty()
        {
            return Ok(());
        }
        phase = (phase + step) % 4800;
    }
    invalid("The selected recurrence dates never occur with this interval and due-date calendar. Choose a different date or interval.")
}

/// Calendar patterns may yield several dates within one period, or none (e.g.
/// February 31 / a missing fifth weekday). Only matching occurrences count.
fn patterned_date(base: NaiveDate, anchor: NaiveDate, rule: &Value) -> Result<Option<NaiveDate>> {
    let interval = rule["interval"].as_u64().expect("validated interval") as u32;
    let yearly = rule["frequency"] == "yearly";
    let month = if yearly {
        rule["year_month"]
            .as_u64()
            .map(|m| m as u32)
            .unwrap_or(anchor.month())
    } else {
        base.month()
    };
    let end = rule["end_date"].as_str().map(date).transpose()?;
    let mut period = NaiveDate::from_ymd_opt(base.year(), month, 1).ok_or_else(out_of_range)?;
    let mut visited = HashSet::new();
    loop {
        if period.year() > 9999 {
            return Ok(None);
        }
        if end.is_some_and(|end| period > end) {
            return Ok(None);
        }
        for candidate in month_candidates(period.year(), period.month(), rule, anchor.day())? {
            if candidate > base {
                return Ok(end.is_none_or(|end| candidate <= end).then_some(candidate));
            }
        }
        if !visited.insert((period.year().rem_euclid(400), period.month())) {
            // Older persisted schedules may predate validation. End the series
            // instead of preventing completion when no successor can exist.
            return Ok(None);
        }
        let Some(next_period) =
            period.checked_add_months(Months::new(if yearly { interval * 12 } else { interval }))
        else {
            return Ok(None);
        };
        period = next_period;
    }
}

/// Return the next scheduled date strictly after the selected anchor, or None
/// when a count/end-date limit ends the series. Completing an overdue occurrence
/// advances exactly one occurrence; it does not silently skip overdue work.
pub(super) fn next_date(due: &str, rule: &Value, completion_date: &str) -> Result<Option<String>> {
    let rule = validate_rule(rule)?;
    let due = date(due)?;
    let completion = date(completion_date)?;
    if rule["count"].as_u64().is_some_and(|count| count <= 1) {
        return Ok(None);
    }
    let completion_based = rule["mode"] == "completion_date";
    let base = if completion_based { completion } else { due };
    let interval = rule["interval"]
        .as_u64()
        .expect("validated recurrence interval");
    let frequency = rule["frequency"]
        .as_str()
        .expect("validated recurrence frequency");
    let calendar_anchor = if completion_based {
        base
    } else {
        match rule["anchor_date"].as_str() {
            Some(value) => date(value)?,
            None => due,
        }
    };
    if has_custom_pattern(&rule) {
        return Ok(patterned_date(base, calendar_anchor, &rule)?
            .map(|date| date.format("%Y-%m-%d").to_string()));
    }
    let next = match frequency {
        "daily" => base
            .checked_add_days(Days::new(interval))
            .ok_or_else(out_of_range)?,
        "weekly" => {
            let weekdays = rule["weekdays"].as_array().expect("validated weekdays");
            if weekdays.is_empty() {
                base.checked_add_days(Days::new(7 * interval))
                    .ok_or_else(out_of_range)?
            } else {
                // ISO weeks begin on Monday. Selected days later in this week
                // still belong to its occurrence; the next eligible week begins
                // exactly `interval` weeks after this one.
                let monday = base
                    .checked_sub_days(Days::new(u64::from(base.weekday().num_days_from_monday())))
                    .ok_or_else(out_of_range)?;
                let mut found = None;
                for offset in 1..=(7 * interval + 7) {
                    let candidate = base
                        .checked_add_days(Days::new(offset))
                        .ok_or_else(out_of_range)?;
                    let weeks_since_monday = candidate.signed_duration_since(monday).num_days() / 7;
                    let weekday = u64::from(candidate.weekday().number_from_monday());
                    if weeks_since_monday % interval as i64 == 0
                        && weekdays.iter().any(|day| day.as_u64() == Some(weekday))
                    {
                        found = Some(candidate);
                        break;
                    }
                }
                found.ok_or_else(out_of_range)?
            }
        }
        "monthly" => {
            let target = base
                .with_day(1)
                .ok_or_else(out_of_range)?
                .checked_add_months(Months::new(interval as u32))
                .ok_or_else(out_of_range)?;
            clamped_day(target.year(), target.month(), calendar_anchor.day())?
        }
        "yearly" => clamped_day(
            base.year()
                .checked_add(interval as i32)
                .ok_or_else(out_of_range)?,
            calendar_anchor.month(),
            calendar_anchor.day(),
        )?,
        _ => unreachable!("frequency was validated"),
    };
    if !(1..=9999).contains(&next.year()) {
        return Err(out_of_range());
    }
    if let Some(end) = rule["end_date"].as_str() {
        if next > date(end)? {
            return Ok(None);
        }
    }
    Ok(Some(next.format("%Y-%m-%d").to_string()))
}

fn timezone(value: &str) -> Result<Tz> {
    value.parse::<Tz>().map_err(|_| {
        ServiceError::Invalid(
            "timezone must be a recognized IANA name, such as Europe/Zurich or UTC.".into(),
        )
    })
}

pub(super) fn validate_timezone(value: &str) -> Result<()> {
    timezone(value).map(|_| ())
}

/// Project a draft into a visible calendar range with the exact successor engine.
/// Completion-based schedules necessarily assume completion on each due date;
/// a future user's real completion date cannot be predicted.
pub(super) fn preview(input: &Map<String, Value>) -> Result<Value> {
    crate::fields(
        input,
        &[
            "due_date",
            "recurrence",
            "timezone",
            "range_start",
            "range_end",
            "limit",
        ],
    )?;
    let mut current = crate::required(input, "due_date")?.to_string();
    let due = date(&current)?;
    let start = date(crate::required(input, "range_start")?)?;
    let end = date(crate::required(input, "range_end")?)?;
    if start > end || end.signed_duration_since(start).num_days() > 365 {
        return invalid("Preview range must contain 1 to 366 calendar days.");
    }
    if let Some(zone) = input.get("timezone") {
        validate_timezone(
            zone.as_str()
                .ok_or_else(|| ServiceError::Invalid("timezone must be an IANA name.".into()))?,
        )?;
    }
    let limit = match input.get("limit") {
        None => 128,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=512).contains(n))
            .ok_or_else(|| {
                ServiceError::Invalid("Preview limit must be an integer from 1 to 512.".into())
            })? as usize,
    };
    let value = input
        .get("recurrence")
        .ok_or_else(|| ServiceError::Invalid("recurrence is required (or null).".into()))?;
    if value.is_null() {
        return Ok(json!({"dates":[],"truncated":false,"assumption":"due_date"}));
    }
    let mut rule = validate_rule(value)?;
    if rule["anchor_date"].is_null() {
        rule["anchor_date"] = json!(current);
    }
    let assumption = if rule["mode"] == "completion_date" {
        "completion_on_due_date"
    } else {
        "due_date"
    };
    let mut dates = Vec::new();
    if due >= end {
        return Ok(json!({"dates":dates,"truncated":false,"assumption":assumption}));
    }
    // Bound work even for a very old unbounded daily series. The caller can
    // distinguish an exhausted calendar from an intentionally bounded preview.
    const MAX_PREVIEW_STEPS: usize = 10_000;
    for _ in 0..MAX_PREVIEW_STEPS {
        let Some(next) = next_date(&current, &rule, &current)? else {
            return Ok(json!({"dates":dates,"truncated":false,"assumption":assumption}));
        };
        let next_day = date(&next)?;
        if next_day > end {
            return Ok(json!({"dates":dates,"truncated":false,"assumption":assumption}));
        }
        if next_day >= start {
            if dates.len() == limit {
                return Ok(json!({"dates":dates,"truncated":true,"assumption":assumption}));
            }
            dates.push(next.clone());
        }
        current = next;
        if let Some(count) = rule["count"].as_u64() {
            rule["count"] = json!(count - 1);
        }
        if next_day == end || rule["count"].as_u64() == Some(1) {
            return Ok(json!({"dates":dates,"truncated":false,"assumption":assumption}));
        }
    }
    Ok(json!({"dates":dates,"truncated":true,"assumption":assumption}))
}

/// Resolve a local wall-clock minute to an unambiguous UTC reminder timestamp.
pub(super) fn local_instant(
    date_value: &str,
    time_value: &str,
    timezone_value: &str,
) -> Result<String> {
    let local_date = date(date_value)?;
    let time = NaiveTime::parse_from_str(time_value, "%H:%M")
        .map_err(|_| ServiceError::Invalid("Local time must use HH:MM (00:00–23:59).".into()))?;
    if time.format("%H:%M").to_string() != time_value {
        return invalid("Local time must use HH:MM.");
    }
    let tz = timezone(timezone_value)?;
    let mut local = local_date.and_time(time);
    loop {
        let resolved = match tz.from_local_datetime(&local) {
            LocalResult::Single(value) => Some(value),
            LocalResult::Ambiguous(first, second) => Some(first.min(second)),
            LocalResult::None => None,
        };
        if let Some(value) = resolved {
            return Ok(value
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true));
        }
        local = local
            .checked_add_signed(Duration::minutes(1))
            .ok_or_else(out_of_range)?;
        if local.date() != local_date {
            return invalid(
                "This local date has no valid time at or after the selected time in that timezone.",
            );
        }
    }
}

/// Keep a reminder's offset from the due wall-clock time across recurrence, even
/// when the elapsed UTC duration between dates changes at a daylight-saving boundary.
pub(super) fn shift_instant_by_due(
    old_instant: &str,
    old_due_date: &str,
    new_due_date: &str,
    due_time: Option<&str>,
    timezone: &str,
) -> Result<String> {
    let reminder = DateTime::parse_from_rfc3339(old_instant)
        .map_err(|_| {
            ServiceError::Invalid("Reminder must be an RFC3339 timestamp with a timezone.".into())
        })?
        .with_timezone(&Utc);
    let time = due_time.unwrap_or("09:00");
    let old_due = DateTime::parse_from_rfc3339(&local_instant(old_due_date, time, timezone)?)
        .map_err(|_| ServiceError::Invalid("Unable to resolve the previous due time.".into()))?
        .with_timezone(&Utc);
    let next_due = DateTime::parse_from_rfc3339(&local_instant(new_due_date, time, timezone)?)
        .map_err(|_| ServiceError::Invalid("Unable to resolve the next due time.".into()))?
        .with_timezone(&Utc);
    let offset = reminder.signed_duration_since(old_due);
    let shifted = next_due
        .checked_add_signed(offset)
        .ok_or_else(out_of_range)?;
    if !(1..=9999).contains(&shifted.year()) {
        return Err(out_of_range());
    }
    Ok(shifted.to_rfc3339_opts(SecondsFormat::Millis, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_pattern_normalization_is_stable_and_rejects_conflicts() {
        let normalized = validate_rule(
            &json!({"frequency":"monthly","month_days":[31,-1,1,31],"skip_weekends":true}),
        )
        .unwrap();
        assert_eq!(normalized["month_days"], json!([-1, 1, 31]));
        assert_eq!(normalized["ordinal_weekday"], Value::Null);
        assert_eq!(validate_rule(&normalized).unwrap(), normalized);
        for rule in [
            json!({"frequency":"monthly","month_days":[0]}),
            json!({"frequency":"monthly","month_days":[32]}),
            json!({"frequency":"monthly","month_days":["1"]}),
            json!({"frequency":"daily","month_days":[1]}),
            json!({"frequency":"yearly","month_days":[1,2]}),
            json!({"frequency":"yearly","month_days":[-1]}),
            json!({"frequency":"monthly","month_days":[1],"workday":"first"}),
            json!({"frequency":"monthly","month_days":[1],"ordinal_weekday":{"ordinal":1,"weekday":1}}),
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":0,"weekday":1}}),
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":6,"weekday":1}}),
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":1,"weekday":8}}),
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":1,"weekday":1,"extra":true}}),
            json!({"frequency":"yearly","workday":"last"}),
            json!({"frequency":"monthly","workday":"middle"}),
            json!({"frequency":"monthly","year_month":1}),
            json!({"frequency":"yearly","year_month":13,"month_days":[1]}),
            json!({"frequency":"yearly","year_month":2,"month_days":[30]}),
            json!({"frequency":"yearly","year_month":4,"month_days":[31]}),
            json!({"frequency":"monthly","skip_weekends":"true"}),
            json!({"frequency":"monthly","skip_weekends":true,"workday":"first"}),
        ] {
            assert!(validate_rule(&rule).is_err(), "accepted {rule}");
        }
    }

    #[test]
    fn monthly_multiple_dates_last_day_and_interval_are_ordered_without_duplicates() {
        let rule = json!({"frequency":"monthly","month_days":[1,15,31,-1],"interval":2});
        for (due, expected) in [
            ("2026-01-01", "2026-01-15"),
            ("2026-01-15", "2026-01-31"),
            ("2026-01-31", "2026-03-01"),
            ("2026-03-15", "2026-03-31"),
            ("2026-03-31", "2026-05-01"),
        ] {
            assert_eq!(
                next_date(due, &rule, due).unwrap().as_deref(),
                Some(expected)
            );
        }
        let strict = json!({"frequency":"monthly","month_days":[31]});
        assert_eq!(
            next_date("2026-01-31", &strict, "2026-01-31")
                .unwrap()
                .as_deref(),
            Some("2026-03-31")
        );
        let last = json!({"frequency":"monthly","month_days":[-1]});
        assert_eq!(
            next_date("2024-01-31", &last, "2024-01-31")
                .unwrap()
                .as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(
            next_date("2024-02-29", &last, "2024-02-29")
                .unwrap()
                .as_deref(),
            Some("2024-03-31")
        );
    }

    #[test]
    fn ordinal_weekdays_skip_missing_fifths_and_workdays_exclude_weekends() {
        let first_monday =
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":1,"weekday":1}});
        assert_eq!(
            next_date("2026-01-05", &first_monday, "2026-01-05")
                .unwrap()
                .as_deref(),
            Some("2026-02-02")
        );
        let fifth_monday =
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":5,"weekday":1}});
        assert_eq!(
            next_date("2026-01-01", &fifth_monday, "2026-01-01")
                .unwrap()
                .as_deref(),
            Some("2026-03-30")
        );
        assert_eq!(
            next_date("2026-03-30", &fifth_monday, "2026-03-30")
                .unwrap()
                .as_deref(),
            Some("2026-06-29")
        );
        let last_friday =
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":-1,"weekday":5}});
        assert_eq!(
            next_date("2026-01-30", &last_friday, "2026-01-30")
                .unwrap()
                .as_deref(),
            Some("2026-02-27")
        );
        for (workday, due, expected) in [
            ("first", "2026-01-01", "2026-02-02"),
            ("last", "2026-01-30", "2026-02-27"),
            ("first", "2026-05-01", "2026-06-01"),
        ] {
            assert_eq!(
                next_date(due, &json!({"frequency":"monthly","workday":workday}), due)
                    .unwrap()
                    .as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn monthly_skip_weekends_omits_occurrences_and_completion_mode_reanchors_period() {
        let rule = json!({"frequency":"monthly","month_days":[-1],"skip_weekends":true});
        assert_eq!(
            next_date("2026-01-30", &rule, "2026-01-30")
                .unwrap()
                .as_deref(),
            Some("2026-03-31")
        );
        let multi = json!({"frequency":"monthly","month_days":[1,15,28],"skip_weekends":true});
        assert_eq!(
            next_date("2026-01-28", &multi, "2026-01-28")
                .unwrap()
                .as_deref(),
            Some("2026-04-01")
        );
        let due = json!({"frequency":"monthly","month_days":[1,15],"interval":2});
        assert_eq!(
            next_date("2026-01-01", &due, "2026-02-10")
                .unwrap()
                .as_deref(),
            Some("2026-01-15")
        );
        let completion = json!({"frequency":"monthly","month_days":[1,15],"interval":2,"mode":"completion_date"});
        assert_eq!(
            next_date("2026-01-01", &completion, "2026-02-10")
                .unwrap()
                .as_deref(),
            Some("2026-02-15")
        );
        assert_eq!(
            next_date("2026-01-01", &completion, "2026-02-15")
                .unwrap()
                .as_deref(),
            Some("2026-04-01")
        );
    }

    #[test]
    fn yearly_calendar_and_ordinal_patterns_obey_intervals_leap_years_and_limits() {
        let leap = json!({"frequency":"yearly","year_month":2,"month_days":[29]});
        assert_eq!(
            next_date("2024-02-29", &leap, "2024-02-29")
                .unwrap()
                .as_deref(),
            Some("2028-02-29")
        );
        let thanksgiving = json!({"frequency":"yearly","year_month":11,"ordinal_weekday":{"ordinal":4,"weekday":4},"interval":2});
        assert_eq!(
            next_date("2026-11-26", &thanksgiving, "2026-11-26")
                .unwrap()
                .as_deref(),
            Some("2028-11-23")
        );
        let fifth = json!({"frequency":"yearly","year_month":2,"ordinal_weekday":{"ordinal":5,"weekday":1}});
        assert_eq!(
            next_date("2026-01-01", &fifth, "2026-01-01")
                .unwrap()
                .as_deref(),
            Some("2044-02-29")
        );
        let until =
            json!({"frequency":"yearly","year_month":2,"month_days":[29],"end_date":"2027-12-31"});
        assert_eq!(next_date("2024-02-29", &until, "2024-02-29").unwrap(), None);
        let last = json!({"frequency":"monthly","month_days":[-1],"end_date":"2026-02-28"});
        assert_eq!(
            next_date("2026-01-31", &last, "2026-01-31")
                .unwrap()
                .as_deref(),
            Some("2026-02-28")
        );
        assert_eq!(next_date("2026-02-28", &last, "2026-02-28").unwrap(), None);
        let one = json!({"frequency":"monthly","workday":"last","count":1});
        assert_eq!(next_date("2026-01-01", &one, "2026-01-01").unwrap(), None);
        assert_eq!(
            next_date(
                "9999-12-31",
                &json!({"frequency":"monthly","month_days":[31]}),
                "9999-12-31"
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn sterile_calendars_are_rejected_but_persisted_rules_end_cleanly() {
        for (due, rule) in [
            (
                "2025-04-30",
                json!({"frequency":"yearly","month_days":[31]}),
            ),
            (
                "2025-04-30",
                json!({"frequency":"monthly","month_days":[31],"interval":12}),
            ),
            (
                "2025-02-28",
                json!({"frequency":"yearly","year_month":2,"month_days":[29],"interval":4}),
            ),
            (
                "2026-02-28",
                json!({"frequency":"yearly","year_month":2,"month_days":[29],"interval":4}),
            ),
            (
                "2027-02-28",
                json!({"frequency":"yearly","year_month":2,"month_days":[29],"interval":4}),
            ),
        ] {
            assert!(
                validate_calendar(due, &rule).is_err(),
                "accepted {due} {rule}"
            );
            assert_eq!(next_date(due, &rule, due).unwrap(), None);
        }
        let valid = json!({"frequency":"yearly","year_month":2,"month_days":[29],"interval":4,"count":1,"end_date":"2024-02-29"});
        assert!(validate_calendar("2024-02-29", &valid).is_ok());
        let completion = json!({"frequency":"yearly","year_month":2,"month_days":[29],"interval":4,"mode":"completion_date"});
        assert!(validate_calendar("2024-02-29", &completion).is_ok());
        assert_eq!(
            next_date("2024-02-29", &completion, "2025-02-28").unwrap(),
            None
        );
        let once_per_cycle = json!({"frequency":"yearly","year_month":2,"ordinal_weekday":{"ordinal":5,"weekday":1},"interval":80});
        assert!(validate_calendar("2044-02-29", &once_per_cycle).is_ok());
        assert_eq!(
            next_date("2044-02-29", &once_per_cycle, "2044-02-29")
                .unwrap()
                .as_deref(),
            Some("2444-02-29")
        );
    }

    #[test]
    fn normalization_is_stable_and_rejects_invalid_fields() {
        let rule = validate_rule(&json!({"frequency":"weekly","weekdays":[5,1,5]})).unwrap();
        assert_eq!(rule["interval"], 1);
        assert_eq!(rule["weekdays"], json!([1, 5]));
        assert_eq!(rule["mode"], "due_date");
        assert_eq!(validate_rule(&rule).unwrap(), rule);
        for invalid in [
            json!("daily"),
            json!({"frequency":"hourly"}),
            json!({"frequency":"daily","interval":0}),
            json!({"frequency":"daily","interval":366}),
            json!({"frequency":"daily","interval":1.5}),
            json!({"frequency":"daily","weekdays":[1]}),
            json!({"frequency":"weekly","weekdays":[0]}),
            json!({"frequency":"weekly","weekdays":[8]}),
            json!({"frequency":"weekly","weekdays":["1"]}),
            json!({"frequency":"daily","count":0}),
            json!({"frequency":"daily","mode":"forever"}),
            json!({"frequency":"daily","end_date":"2026-02-30"}),
            json!({"frequency":"daily","anchor_date":"bad"}),
            json!({"frequency":"daily","unknown":true}),
        ] {
            assert!(validate_rule(&invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn daily_intervals_limits_and_completion_based_dates() {
        assert_eq!(
            next_date(
                "2026-10-27",
                &json!({"frequency":"daily","interval":3}),
                "2026-11-02"
            )
            .unwrap(),
            Some("2026-10-30".into())
        );
        assert_eq!(
            next_date(
                "2026-10-27",
                &json!({"frequency":"daily","interval":3,"mode":"completion_date"}),
                "2026-11-02"
            )
            .unwrap(),
            Some("2026-11-05".into())
        );
        assert_eq!(
            next_date(
                "2026-10-27",
                &json!({"frequency":"daily","count":1}),
                "2026-10-27"
            )
            .unwrap(),
            None
        );
        assert_eq!(
            next_date(
                "2026-10-27",
                &json!({"frequency":"daily","end_date":"2026-10-27"}),
                "2026-10-27"
            )
            .unwrap(),
            None
        );
        assert_eq!(
            next_date(
                "2026-10-27",
                &json!({"frequency":"daily","end_date":"2026-10-28"}),
                "2026-10-27"
            )
            .unwrap(),
            Some("2026-10-28".into())
        );
    }

    #[test]
    fn weekly_selected_days_honor_iso_week_and_interval() {
        let rule = json!({"frequency":"weekly","interval":2,"weekdays":[1,3,5]});
        assert_eq!(
            next_date("2026-10-26", &rule, "2026-10-26").unwrap(),
            Some("2026-10-28".into())
        );
        assert_eq!(
            next_date("2026-10-28", &rule, "2026-10-28").unwrap(),
            Some("2026-10-30".into())
        );
        assert_eq!(
            next_date("2026-10-30", &rule, "2026-10-30").unwrap(),
            Some("2026-11-09".into())
        );
        assert_eq!(
            next_date("2026-11-01", &rule, "2026-11-01").unwrap(),
            Some("2026-11-09".into())
        );
        assert_eq!(
            next_date(
                "2026-10-27",
                &json!({"frequency":"weekly","interval":2}),
                "2026-10-27"
            )
            .unwrap(),
            Some("2026-11-10".into())
        );
    }

    #[test]
    fn weekly_completion_mode_reanchors_to_actual_completion_week() {
        let rule =
            json!({"frequency":"weekly","interval":2,"weekdays":[1,5],"mode":"completion_date"});
        assert_eq!(
            next_date("2026-10-26", &rule, "2026-11-04").unwrap(),
            Some("2026-11-06".into())
        );
        assert_eq!(
            next_date("2026-10-26", &rule, "2026-11-06").unwrap(),
            Some("2026-11-16".into())
        );
    }

    #[test]
    fn monthly_clamps_then_recovers_original_day_with_anchor() {
        let rule = json!({"frequency":"monthly","anchor_date":"2026-01-31"});
        assert_eq!(
            next_date("2026-01-31", &rule, "2026-01-31").unwrap(),
            Some("2026-02-28".into())
        );
        assert_eq!(
            next_date("2026-02-28", &rule, "2026-02-28").unwrap(),
            Some("2026-03-31".into())
        );
        assert_eq!(
            next_date("2026-12-31", &rule, "2026-12-31").unwrap(),
            Some("2027-01-31".into())
        );
        assert_eq!(
            next_date(
                "2026-01-31",
                &json!({"frequency":"monthly","interval":2}),
                "2026-01-31"
            )
            .unwrap(),
            Some("2026-03-31".into())
        );
    }

    #[test]
    fn yearly_clamps_then_recovers_leap_day_and_completion_mode_ignores_anchor() {
        let rule = json!({"frequency":"yearly","anchor_date":"2024-02-29"});
        assert_eq!(
            next_date("2024-02-29", &rule, "2024-02-29").unwrap(),
            Some("2025-02-28".into())
        );
        assert_eq!(
            next_date("2027-02-28", &rule, "2027-02-28").unwrap(),
            Some("2028-02-29".into())
        );
        assert_eq!(
            next_date(
                "2026-01-31",
                &json!({"frequency":"monthly","anchor_date":"2026-01-31","mode":"completion_date"}),
                "2026-02-12"
            )
            .unwrap(),
            Some("2026-03-12".into())
        );
    }

    #[test]
    fn calendar_limits_return_errors_without_panics() {
        assert!(next_date("9999-12-31", &json!({"frequency":"daily"}), "9999-12-31").is_err());
        assert!(next_date("9999-12-01", &json!({"frequency":"monthly"}), "9999-12-01").is_err());
        assert!(next_date("9999-01-01", &json!({"frequency":"yearly"}), "9999-01-01").is_err());
        assert!(next_date("bad", &json!({"frequency":"daily"}), "2026-10-27").is_err());
    }

    #[test]
    fn local_time_handles_dst_gap_and_fold_explicitly() {
        assert_eq!(
            local_instant("2026-03-29", "02:30", "Europe/Zurich").unwrap(),
            "2026-03-29T01:00:00.000Z"
        );
        assert_eq!(
            local_instant("2026-10-25", "02:30", "Europe/Zurich").unwrap(),
            "2026-10-25T00:30:00.000Z"
        );
        assert_eq!(
            local_instant("2026-10-27", "09:00", "Europe/Zurich").unwrap(),
            "2026-10-27T08:00:00.000Z"
        );
        assert!(local_instant("2011-12-30", "09:00", "Pacific/Apia").is_err());
        assert!(local_instant("2026-10-27", "24:00", "Europe/Zurich").is_err());
        assert!(local_instant("2026-10-27", "9:00", "Europe/Zurich").is_err());
        assert!(local_instant("2026-10-27", "09:00", "Not/AZone").is_err());
        assert!(validate_timezone("Europe/Zurich").is_ok());
        assert!(validate_timezone("UTC").is_ok());
    }

    #[test]
    fn reminder_offset_preserved_across_spring_and_fall_changes() {
        assert_eq!(
            shift_instant_by_due(
                "2026-03-28T07:45:00Z",
                "2026-03-28",
                "2026-03-29",
                Some("09:00"),
                "Europe/Zurich"
            )
            .unwrap(),
            "2026-03-29T06:45:00.000Z"
        );
        assert_eq!(
            shift_instant_by_due(
                "2026-10-24T06:45:00Z",
                "2026-10-24",
                "2026-10-25",
                Some("09:00"),
                "Europe/Zurich"
            )
            .unwrap(),
            "2026-10-25T07:45:00.000Z"
        );
        assert_eq!(
            shift_instant_by_due(
                "2026-10-27T08:00:00Z",
                "2026-10-27",
                "2026-10-28",
                None,
                "Europe/Zurich"
            )
            .unwrap(),
            "2026-10-28T08:00:00.000Z"
        );
        assert!(
            shift_instant_by_due("bad", "2026-10-27", "2026-10-28", None, "Europe/Zurich").is_err()
        );
        // Positive offsets are just as valid as reminders before the due time.
        assert_eq!(
            shift_instant_by_due(
                "2026-03-28T08:30:00Z",
                "2026-03-28",
                "2026-03-29",
                Some("09:00"),
                "Europe/Zurich"
            )
            .unwrap(),
            "2026-03-29T07:30:00.000Z"
        );
        assert_eq!(
            shift_instant_by_due(
                "2024-02-28T08:00:00Z",
                "2024-02-29",
                "2025-02-28",
                Some("09:00"),
                "Europe/Zurich"
            )
            .unwrap(),
            "2025-02-27T08:00:00.000Z"
        );
    }
}
