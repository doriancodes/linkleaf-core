use anyhow::Result;
use tempfile::tempdir;

use linkleaf_core::{add, linkleaf_proto::Summary, linkleaf_proto::Via, list};
use time::{OffsetDateTime, UtcOffset};

fn main() -> Result<()> {
    #[cfg(feature = "logs")]
    {
        use tracing_subscriber::{EnvFilter, fmt};
        let _ = fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .try_init(); // ignore "already set" in tests
    }

    let dir = tempdir()?;
    let file = dir.path().join("feed.pb");

    let _a = add(
        file.clone(),
        "Tokio - Asynchronous Rust",
        "https://tokio.rs/".into(),
        Some(Summary::new("A runtime for reliable async apps")),
        Some("rust, async, tokio".into()),
        Some(Via::new("website")),
        None, // generate id
    )?;

    // list everything
    let feed = list(&file, None, None)?;
    println!("feed version: {}", feed.version);
    println!("links: {}", feed.links.len());
    for (i, l) in feed.links.iter().enumerate() {
        println!("{i}: {} [{}]  {}", l.title, l.id, l.url);
    }

    // show today's date we wrote (for reference)
    let today = OffsetDateTime::now_utc()
        .to_offset(UtcOffset::current_local_offset()?)
        .date();
    println!("today (local): {today}");

    Ok(())
}
