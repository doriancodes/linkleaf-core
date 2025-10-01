use anyhow::Result;
use tempfile::tempdir;

use linkleaf_core::{add, linkleaf_proto::DateTime, list};
use time::Month;
use time::{OffsetDateTime, UtcOffset};

fn from_month(value: Month) -> i32 {
    match value {
        Month::January => 1,
        Month::February => 2,
        Month::March => 3,
        Month::April => 4,
        Month::May => 5,
        Month::June => 6,
        Month::July => 7,
        Month::August => 8,
        Month::September => 9,
        Month::October => 10,
        Month::November => 11,
        Month::December => 12,
    }
}

fn to_proto_datetime(offsetdatetime: &OffsetDateTime) -> Result<DateTime> {
    Ok(DateTime {
        year: offsetdatetime.year(),
        month: from_month(offsetdatetime.month()),
        day: offsetdatetime.day().try_into()?,
        hours: offsetdatetime.hour().try_into()?,
        minutes: offsetdatetime.minute().try_into()?,
        seconds: offsetdatetime.second().try_into()?,
        nanos: offsetdatetime.nanosecond().try_into()?,
    })
}

fn main() -> Result<()> {
    let dir = tempdir()?;
    let file = dir.path().join("feed.pb");

    // Seed some links (dates are set to "now local" internally)
    let _ = add(
        file.clone(),
        "A",
        "https://a/".into(),
        None,
        Some("rust, async".into()),
        None,
        None,
    )?;
    let _ = add(
        file.clone(),
        "B",
        "https://b/".into(),
        None,
        Some("tokio".into()),
        None,
        None,
    )?;
    let _ = add(
        file.clone(),
        "C",
        "https://c/".into(),
        None,
        Some("db, rust".into()),
        None,
        None,
    )?;

    // Filter by tag (case-insensitive, any-of)
    let rust_only = list(&file, Some(vec!["RUST".into()]), None)?;
    println!("rust_only: {}", rust_only.links.len());
    for l in &rust_only.links {
        println!("- {}", l.title);
    }

    // Filter by date
    let today = OffsetDateTime::now_utc().to_offset(UtcOffset::current_local_offset()?);
    let today = to_proto_datetime(&today)?;
    let today_only = list(&file, None, Some(today))?;
    println!("today_only: {}", today_only.links.len());

    Ok(())
}
