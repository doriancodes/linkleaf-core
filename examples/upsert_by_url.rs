use anyhow::Result;
use tempfile::tempdir;

use linkleaf_core::{add, linkleaf_proto::Summary, list};

fn main() -> Result<()> {
    let dir = tempdir()?;
    let file = dir.path().join("feed.pb");

    let a = add(
        file.clone(),
        "Original",
        "https://same.url/".into(),
        None,
        Some("t1".into()),
        None,
        None,
    )?;

    // Same URL + id=None -> update the existing entry (moved to front)
    let a2 = add(
        file.clone(),
        "Original (updated)",
        "https://same.url/".into(),
        Some(Summary::new("updated")),
        Some("t2".into()),
        None,
        None,
    )?;

    assert_eq!(a.id, a2.id);

    let feed = list(&file, None, None)?;
    println!(
        "front item: {} [{}] tags: {:?}",
        feed.links[0].title, feed.links[0].id, feed.links[0].tags
    );

    Ok(())
}
