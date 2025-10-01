pub mod fs;
pub mod validation;
pub mod linkleaf_proto {
    include!(concat!(env!("OUT_DIR"), "/linkleaf.v1.rs"));
}

use crate::fs::{read_feed, write_feed};
use crate::linkleaf_proto::{DateTime, Feed, Link, Summary, Via};
use anyhow::Result;
use chrono::{FixedOffset, TimeZone};
use rss::{CategoryBuilder, ChannelBuilder, GuidBuilder, Item, ItemBuilder};
use std::path::Path;
use time::Month;
use time::OffsetDateTime;
use uuid::Uuid;

fn is_not_found(err: &anyhow::Error) -> bool {
    err.downcast_ref::<std::io::Error>()
        .map(|e| e.kind() == std::io::ErrorKind::NotFound)
        .unwrap_or(false)
}

fn update_link_in_place(
    feed: &mut Feed,
    pos: usize,
    title: String,
    url: String,
    date: Option<DateTime>,
    summary: Option<Summary>,
    tags: Vec<String>,
    via: Option<Via>,
) -> Link {
    // take ownership, mutate, then reinsert at front
    let mut item = feed.links.remove(pos);
    item.title = title;
    item.url = url;
    item.datetime = date;
    item.summary = summary;
    item.tags = tags;
    item.via = via;

    feed.links.insert(0, item.clone());
    item
}

fn insert_new_link_front(
    feed: &mut Feed,
    id: String,
    title: String,
    url: String,
    datetime: Option<DateTime>,
    summary: Option<Summary>,
    tags: Vec<String>,
    via: Option<Via>,
) -> Link {
    let link = Link {
        summary: summary,
        tags, // field init shorthand
        via: via,
        id,
        title,
        url,
        datetime,
    };
    feed.links.insert(0, link.clone());
    link
}

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

/// Add or update a link in a protobuf feed file, then persist the feed.
///
/// ## Behavior
/// - Reads the feed at `file`. If it doesn't exist, a new feed is initialized (`version = 1`).
/// - If an `id` is provided:
///   - Updates the existing link with that `id` if found (title, url, summary, tags, via),
///     sets its `date` to **today (local datetime, `YYYY-MM-DD HH:MM:SS`)**, and moves it
///     to the **front** (newest-first).
///   - Otherwise inserts a **new** link at the front with that explicit `id`.
/// - If no `id` is provided:
///   - Updates the first link whose `url` matches; sets `date` to today and moves it to the front.
///   - Otherwise inserts a **new** link at the front with a freshly generated UUID v4 `id`.
///
/// Persists the entire feed by calling `write_feed`, which writes atomically
/// via a temporary file and `rename`.
///
/// ## Arguments
/// - `file`: Path to the `.pb` feed file to update/create.
/// - `title`: Human-readable title for the link.
/// - `url`: Target URL for the link.
/// - `summary`: Optional blurb/notes (`None` -> empty string).
/// - `tags`: Zero or more tags as an **iterator of strings** (e.g., `["rust", "async", "tokio"]`).
/// - `via`: Optional source/attribution (`None` -> empty string).
/// - `id`: Optional stable identifier. If present, performs an **upsert** by `id`.
///
/// ## Returns
/// The newly created or updated [`Link`].
///
/// ## Ordering
/// Links are kept **newest-first**; both inserts and updates end up at index `0`.
///
/// ## Errors
/// - Propagates any error from `read_feed` (except “not found”, which initializes a new feed).
/// - Propagates any error from `write_feed`.
/// - No inter-process locking is performed; concurrent writers may race.
///
/// ## Example
/// ```no_run
/// use std::path::PathBuf;
/// use linkleaf_core::*;
/// use linkleaf_core::linkleaf_proto::Summary;
/// use uuid::Uuid;
///
/// let file = PathBuf::from("mylinks.pb");
///
/// // Create a new link
/// let a = add(
///     file.clone(),
///     "Tokio - Asynchronous Rust",
///     "https://tokio.rs/",
///     None,
///     ["rust", "async", "tokio"],
///     None,
///     None, // no id -> create (may update if URL already exists)
/// )?;
///
/// // Update the same link by id (upsert)
/// let _id = Uuid::parse_str(&a.id)?;
/// let a2 = add(
///     file.clone(),
///     "Tokio • Async Rust",
///     "https://tokio.rs/",
///     Some(Summary::new("A runtime for reliable async apps")),
///     [],                 // no tags change
///     None,
///     Some(_id),          // provide id -> update or insert with that id
/// )?;
///
/// assert_eq!(a2.id, a.id);
/// Ok::<(), anyhow::Error>(())
/// // After update, the item is at the front (index 0).
/// ```
///
/// ## Notes
/// - Providing an `id` gives the item a stable identity; updates by `id` will also update
///   the stored `url` to the new value you pass.
/// - `date` is always set to “today” in local time on both create and update.
pub fn add<P, S, T>(
    file: P,
    title: S,
    url: S,
    summary: Option<Summary>,
    tags: T,
    via: Option<Via>,
    id: Option<Uuid>,
) -> Result<Link>
where
    P: AsRef<Path>,
    S: Into<String>,
    T: IntoIterator<Item = S>,
{
    let file = file.as_ref();
    // compute local timestamp once
    let local_now = OffsetDateTime::now_local()
        .map_err(|e| anyhow::anyhow!("failed to get local time offset: {e}"))?;

    let datetime = DateTime {
        year: local_now.year() as i32,
        month: from_month(local_now.month()),
        day: local_now.day() as i32,
        hours: local_now.hour() as i32,
        minutes: local_now.minute() as i32,
        seconds: local_now.second() as i32,
        nanos: local_now.nanosecond() as i32,
    };

    // read or init feed
    let mut feed = match read_feed(file) {
        Ok(f) => f,
        Err(err) if is_not_found(&err) => {
            let mut f = Feed::default();
            f.version = 1;
            f
        }
        Err(err) => return Err(err),
    };

    let title = title.into();
    let url = url.into();
    let summary = summary.map(Into::into);
    let via = via.map(Into::into);
    let tags: Vec<String> = tags.into_iter().map(Into::into).collect();
    let id_opt: Option<String> = id.map(|u| u.to_string());

    // behavior:
    // - If `id` provided: update by id; else insert (even if URL duplicates).
    // - If no `id`: update by URL; else insert with fresh UUID.
    let updated_or_new = match id_opt {
        Some(uid) => {
            if let Some(pos) = feed.links.iter().position(|l| l.id == uid) {
                let item = update_link_in_place(
                    &mut feed,
                    pos,
                    title,
                    url,
                    Some(datetime),
                    summary,
                    tags,
                    via,
                );
                #[cfg(feature = "logs")]
                tracing::info!(id = %item.id, "updated existing link by id");
                item
            } else {
                let item = insert_new_link_front(
                    &mut feed,
                    uid,
                    title,
                    url,
                    Some(datetime),
                    summary,
                    tags,
                    via,
                );
                #[cfg(feature = "logs")]
                tracing::info!(id = %item.id, "inserted new link with explicit id");
                item
            }
        }
        None => {
            if let Some(pos) = feed.links.iter().position(|l| l.url == url) {
                let item = update_link_in_place(
                    &mut feed,
                    pos,
                    title,
                    url,
                    Some(datetime),
                    summary,
                    tags,
                    via,
                );
                #[cfg(feature = "logs")]
                tracing::info!(id = %item.id, "inserted new link with explicit id");
                item
            } else {
                let uid = Uuid::new_v4().to_string();
                let item = insert_new_link_front(
                    &mut feed,
                    uid,
                    title,
                    url,
                    Some(datetime),
                    summary,
                    tags,
                    via,
                );
                #[cfg(feature = "logs")]
                tracing::info!(id = %item.id, "inserted new link with explicit id");
                item
            }
        }
    };

    let _modified_feed = write_feed(&file, feed)?;
    #[cfg(feature = "logs")]
    tracing::debug!(links = _modified_feed.links.len(), path = %file.display(), "feed written");

    Ok(updated_or_new)
}

/// Read and return the feed stored in a protobuf file.
///
/// ## Behavior
/// Calls [`read_feed`] on the provided path and returns the parsed [`Feed`]. If tags and/or
/// date filters are provided it filters the resulting [`Feed`].
///
/// ## Arguments
/// - `file`: Path to the `.pb` feed file.
///
/// ## Returns
/// The parsed [`Feed`] on success.
///
/// ## Errors
/// Any error bubbled up from [`read_feed`], e.g. I/O errors (file missing,
/// permissions), or decode errors if the file is not a valid feed.
///
/// ## Example
/// ```no_run
/// use std::path::PathBuf;
/// use linkleaf_core::*;
///
/// let path = PathBuf::from("mylinks.pb");
/// let feed = list(&path, None, None)?;
/// println!("Title: {}, links: {}", feed.title, feed.links.len());
/// Ok::<(), anyhow::Error>(())
/// ```
pub fn list<P: AsRef<Path>>(
    file: P,
    tags: Option<Vec<String>>,
    datetime: Option<DateTime>,
) -> Result<Feed> {
    let file = file.as_ref();
    let mut feed = read_feed(file)?;

    let tag_norms: Option<Vec<String>> = tags.map(|ts| {
        ts.iter()
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    });

    let date_filter: Option<&DateTime> = datetime.as_ref();

    feed.links.retain(|l| {
        let tag_ok = match &tag_norms {
            Some(needles) => l
                .tags
                .iter()
                .any(|t| needles.iter().any(|n| t.eq_ignore_ascii_case(n))),
            None => true,
        };

        let date_ok = match date_filter {
            Some(p) => l.datetime.as_ref().map(|dt| dt == p).unwrap_or(false),
            None => true,
        };

        tag_ok && date_ok
    });

    Ok(feed)
}

impl DateTime {
    /// Converts this `DateTime` to an RFC 2822 string.
    ///
    /// Returns `None` if any field is invalid (e.g., month > 12, day > 31).
    #[allow(deprecated)]
    pub fn to_rfc2822(&self) -> Option<String> {
        // Convert i32 fields to u32 safely
        let month = u32::try_from(self.month).ok()?; // 1..=12
        let day = u32::try_from(self.day).ok()?; // 1..=31
        let hours = u32::try_from(self.hours).ok()?; // 0..=23
        let minutes = u32::try_from(self.minutes).ok()?; // 0..=59
        let seconds = u32::try_from(self.seconds).ok()?; // 0..=60 for leap seconds

        let dt = FixedOffset::east_opt(0) // UTC;
            .map(|d| {
                d.ymd(self.year, month, day)
                    .and_hms(hours, minutes, seconds)
            })?;

        Some(dt.to_rfc2822())
    }
}

fn to_datetime(proto_datetime: &Option<DateTime>) -> Option<String> {
    proto_datetime.as_ref().and_then(|dt| dt.to_rfc2822())
}

/// Converts a `Feed` into an RSS 2.0 XML string.
///
/// This function generates a fully formatted RSS feed from the provided `Feed`
/// data structure. Each `Link` in the feed is converted to an `Item` in the
/// RSS feed using `link_to_rss_item`.
///
/// # Parameters
///
/// - `feed`: A reference to a `Feed` struct containing the feed title and links.
/// - `site_title`: A fallback title for the feed if `feed.title` is empty.
/// - `site_link`: The canonical URL of the website or feed source; used as the
///   `<link>` of the channel.
///
/// # Returns
///
/// Returns a `Result<String>` containing the RSS XML string if successful.
///
/// # Errors
///
/// Returns an error if:
/// - The channel cannot be serialized into XML.
/// - The resulting UTF-8 string cannot be created from the XML buffer.
///
/// # Behavior
///
/// - If `feed.title` is empty, `site_title` is used as the RSS channel title.
/// - Each link's tags, summary, guid, and publication date are included in
///   the corresponding RSS `<item>`.
/// - The XML is pretty-printed with an indentation of 2 spaces.
///
/// # Example
///
/// ```rust
/// use linkleaf_core::linkleaf_proto::Feed;
/// use linkleaf_core::feed_to_rss_xml;
///
/// let feed = Feed {
///     title: "My Links".to_string(),
///     links: vec![/* ... */],
///     version: 1
/// };
/// let rss_xml = feed_to_rss_xml(&feed, "Default Site", "https://example.com")
///     .expect("Failed to generate RSS XML");
/// println!("{}", rss_xml);
/// ```
pub fn feed_to_rss_xml(feed: &Feed, site_title: &str, site_link: &str) -> Result<String> {
    let items: Vec<Item> = feed.links.iter().map(|l| link_to_rss_item(l)).collect();
    let description = format!("Feed about {} generated through Linkleaf", &feed.title);

    let channel = ChannelBuilder::default()
        .title(if feed.title.is_empty() {
            site_title.to_string()
        } else {
            feed.title.clone()
        })
        .link(site_link.to_string())
        .description(description) // if you have it; else set a default
        .items(items)
        .build();

    let mut buf = Vec::new();
    channel.pretty_write_to(&mut buf, b' ', 2)?;
    Ok(String::from_utf8(buf)?)
}

fn link_to_rss_item(l: &Link) -> Item {
    let cats = l
        .tags
        .iter()
        .map(|t| CategoryBuilder::default().name(t.clone()).build())
        .collect::<Vec<_>>();

    ItemBuilder::default()
        .title(Some(l.title.clone()))
        .link(Some(l.url.clone()))
        .description(l.summary.as_ref().map(|c| c.content.clone()))
        .categories(cats)
        .guid(Some(
            GuidBuilder::default()
                .value(format!("urn:uuid:{}", l.id))
                .permalink(false)
                .build(),
        ))
        .pub_date(to_datetime(&l.datetime))
        .build()
}

impl Summary {
    /// Creates a new `Summary` instance with the given content.
    ///
    /// # Parameters
    ///
    /// - `content`: A string slice containing the summary text. Can be empty.
    ///
    /// # Returns
    ///
    /// A `Summary` instance containing the provided content.
    ///
    /// # Example
    ///
    /// ```rust
    /// use linkleaf_core::linkleaf_proto::Summary;
    ///
    /// let summary = Summary::new("This is a brief description of the link.");
    /// assert_eq!(summary.content, "This is a brief description of the link.");
    /// ```
    pub fn new(content: &str) -> Self {
        Summary {
            content: content.into(),
        }
    }
}

impl Via {
    /// Creates a new `Via` instance with the given URL.
    ///
    /// # Parameters
    ///
    /// - `url`: A string slice containing the URL where the link was originally shared.
    ///
    /// # Returns
    ///
    /// A `Via` instance containing the provided URL.
    ///
    /// # Example
    ///
    /// ```rust
    /// use linkleaf_core::linkleaf_proto::Via;
    ///
    /// let via = Via::new("https://example.com/source");
    /// assert_eq!(via.url, "https://example.com/source");
    /// ```
    pub fn new(url: &str) -> Self {
        Via { url: url.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::{add, feed_to_rss_xml, link_to_rss_item, list};
    use crate::fs::{read_feed, write_feed};
    use crate::linkleaf_proto::{DateTime, Feed, Link, Summary, Via};
    use anyhow::Result;
    use tempfile::tempdir;
    use uuid::Uuid;

    // ---- helpers -------------------------------------------------------------

    fn mk_link(
        id: &str,
        title: &str,
        url: &str,
        date_s: DateTime,
        tags: &[&str],
        summary: &str,
        via: &str,
    ) -> Link {
        let _summary = Some(Summary::new(summary));

        let _via = Some(Via::new(via));

        Link {
            id: id.to_string(),
            title: title.to_string(),
            url: url.to_string(),
            datetime: Some(date_s),
            summary: _summary,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            via: _via,
        }
    }

    fn mk_feed(links: Vec<Link>) -> Feed {
        let mut f = Feed::default();
        f.version = 1;
        f.links = links;
        f
    }

    fn sample_link() -> Link {
        Link {
            id: "1234".to_string(),
            title: "Example Post".to_string(),
            url: "https://example.com/post".to_string(),
            summary: Some(Summary::new("This is a summary")),
            tags: vec!["rust".to_string(), "rss".to_string()],
            via: None,
            datetime: Some(DateTime {
                year: 2025,
                month: 10,
                day: 1,
                hours: 14,
                minutes: 30,
                seconds: 45,
                nanos: 00,
            }),
        }
    }

    fn sample_feed() -> Feed {
        Feed {
            title: "Test Feed".to_string(),
            links: vec![sample_link()],
            version: 1,
        }
    }

    // ---- tests ---------------------------------------------------------------

    #[test]
    fn add_creates_file_and_initializes_feed() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");

        // via=None & tags string -> defaults + parse_tags used internally
        let created = add(
            file.clone(),
            "Tokio",
            "https://tokio.rs/".into(),
            None, // summary -> ""
            vec!["rust", "async", "tokio"],
            None,         // via -> ""
            None::<Uuid>, // id -> generated
        )?;

        // File exists and can be read; version initialized to 1
        let feed = read_feed(&file)?;
        assert_eq!(feed.version, 1);
        assert_eq!(feed.links.len(), 1);
        let l = &feed.links[0];
        assert_eq!(l.id, created.id);
        assert_eq!(l.title, "Tokio");
        assert_eq!(l.url, "https://tokio.rs/");
        assert_eq!(l.summary, None);
        assert_eq!(l.via, None);
        assert_eq!(l.tags, vec!["rust", "async", "tokio"]);

        // ID is a valid UUID
        let _ = Uuid::parse_str(&created.id).expect("id should be a valid UUID");
        Ok(())
    }

    #[test]
    fn add_with_explicit_id_inserts_with_given_id() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");
        let wanted = Uuid::new_v4();

        let created = add(
            file.clone(),
            "A",
            "https://a.example/".into(),
            Some(Summary::new("hi")),
            Some("x,y".into()),
            Some(Via::new("via")),
            Some(wanted),
        )?;

        assert_eq!(created.id, wanted.to_string());

        // list(None, None) returns everything; first item is the one we just added
        let feed = list(&file, None, None)?;
        assert_eq!(feed.links.len(), 1);
        assert_eq!(feed.links[0].id, wanted.to_string());
        Ok(())
    }

    #[test]
    fn add_update_by_id_moves_to_front_and_updates_fields() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");
        let tags = ["alpha"];
        // Seed with two links
        let a = add(
            file.clone(),
            "First",
            "https://one/".into(),
            None,
            tags,
            None,
            None::<Uuid>,
        )?;
        let _b = add(
            file.clone(),
            "Second",
            "https://two/".into(),
            None,
            Some("beta".into()),
            None,
            None,
        )?;

        // Update by id of 'a': title/url/tags/via/summary overwritten, item moves to front
        let updated = add(
            file.clone(),
            "First (updated)",
            "https://one-new/".into(),
            Some(Summary::new("note")),
            ["rust", "updated"],
            Some(Via::new("HN")),
            Some(Uuid::parse_str(&a.id)?),
        )?;
        assert_eq!(updated.id, a.id);
        assert_eq!(updated.title, "First (updated)");
        assert_eq!(updated.url, "https://one-new/");
        assert_eq!(updated.summary, Some(Summary::new("note")));
        assert_eq!(updated.via, Some(Via::new("HN")));
        assert_eq!(updated.tags, vec!["rust", "updated"]);

        let feed = list(&file, None, None)?;
        assert_eq!(feed.links.len(), 2);
        assert_eq!(feed.links[0].id, a.id, "updated item should be at index 0");
        assert_eq!(feed.links[0].title, "First (updated)");
        Ok(())
    }

    #[test]
    fn add_update_by_url_when_id_absent() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");

        let first = add(
            file.clone(),
            "Original",
            "https://same.url/".into(),
            None,
            None,
            None,
            None,
        )?;

        // Same URL, id=None => update-in-place (but moved to front) and id stays the same
        let updated = add(
            file.clone(),
            "Original (updated)",
            "https://same.url/".into(),
            Some(Summary::new("s")),
            ["t1", "t2"],
            None,
            None,
        )?;
        assert_eq!(updated.id, first.id);

        let feed = list(&file, None, None)?;
        assert_eq!(feed.links.len(), 1);
        assert_eq!(feed.links[0].title, "Original (updated)");
        assert_eq!(feed.links[0].tags, vec!["t1", "t2"]);
        Ok(())
    }

    #[test]
    fn add_inserts_new_when_url_diff_and_id_absent() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");

        let _a = add(
            file.clone(),
            "A",
            "https://a/".into(),
            None,
            None,
            None,
            None,
        )?;
        let b = add(
            file.clone(),
            "B",
            "https://b/".into(),
            None,
            None,
            None,
            None,
        )?;

        let feed = list(&file, None, None)?;
        assert_eq!(feed.links.len(), 2);
        assert_eq!(feed.links[0].id, b.id, "new item should be at front");
        Ok(())
    }

    #[test]
    fn add_returns_error_on_corrupt_feed() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");

        // Write junk so read_feed(file) inside add() fails with decode error.
        std::fs::write(&file, b"not a protobuf")?;

        let err = add(
            file.clone(),
            "X",
            "https://x/".into(),
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        // Just assert it is an error; message content is from read_feed context.
        assert!(!err.to_string().is_empty());
        Ok(())
    }

    #[test]
    fn list_without_filters_returns_all() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");

        let dt1 = DateTime {
            year: 2025,
            month: 1,
            day: 2,
            hours: 12,
            minutes: 0,
            seconds: 0,
            nanos: 0,
        };

        let dt2 = DateTime {
            year: 2025,
            month: 1,
            day: 3,
            hours: 9,
            minutes: 30,
            seconds: 15,
            nanos: 0,
        };

        // Build a feed directly so we control dates/tags precisely
        let l1 = mk_link("1", "One", "https://1/", dt1, &["rust", "async"], "", "");
        let l2 = mk_link("2", "Two", "https://2/", dt2, &["tokio"], "", "");
        write_feed(&file, mk_feed(vec![l2.clone(), l1.clone()]))?;

        let feed = list(&file, None, None)?;
        assert_eq!(feed.links.len(), 2);
        // Order is preserved from the stored feed for list()
        assert_eq!(feed.links[0].id, l2.id);
        assert_eq!(feed.links[1].id, l1.id);
        Ok(())
    }

    #[test]
    fn list_filters_by_tag_case_insensitive_any_match() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");

        let dt1 = DateTime {
            year: 2025,
            month: 1,
            day: 2,
            hours: 12,
            minutes: 0,
            seconds: 0,
            nanos: 0,
        };

        let dt2 = DateTime {
            year: 2025,
            month: 1,
            day: 3,
            hours: 9,
            minutes: 30,
            seconds: 15,
            nanos: 0,
        };

        let l1 = mk_link("1", "One", "https://1/", dt1, &["rust", "async"], "", "");
        let l2 = mk_link(
            "2",
            "Two",
            "https://2/",
            dt2,
            &["Tokio"], // mixed case
            "",
            "",
        );
        write_feed(&file, mk_feed(vec![l1.clone(), l2.clone()]))?;

        // ANY-of semantics, case-insensitive
        let feed_tokio = list(&file, Some(vec!["tokio".into()]), None)?;
        assert_eq!(feed_tokio.links.len(), 1);
        assert_eq!(feed_tokio.links[0].id, l2.id);

        let feed_async = list(&file, Some(vec!["ASYNC".into()]), None)?;
        assert_eq!(feed_async.links.len(), 1);
        assert_eq!(feed_async.links[0].id, l1.id);

        // Multiple needles -> still "any"
        let feed_multi = list(&file, Some(vec!["zzz".into(), "rust".into()]), None)?;
        assert_eq!(feed_multi.links.len(), 1);
        assert_eq!(feed_multi.links[0].id, l1.id);

        Ok(())
    }

    #[test]
    fn list_filters_by_exact_date_component() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("feed.pb");

        let dt1 = DateTime {
            year: 2025,
            month: 1,
            day: 3,
            hours: 12,
            minutes: 0,
            seconds: 0,
            nanos: 0,
        };

        let dt2 = DateTime {
            year: 2025,
            month: 1,
            day: 3,
            hours: 23,
            minutes: 59,
            seconds: 59,
            nanos: 0,
        };

        let l1 = mk_link("1", "Jan02", "https://1/", dt1, &[], "", "");
        let l2 = mk_link("2", "Jan03", "https://2/", dt2, &[], "", "");
        write_feed(&file, mk_feed(vec![l1.clone(), l2.clone()]))?;

        let filtered = list(&file, None, Some(dt2))?;
        assert_eq!(filtered.links.len(), 1);
        assert_eq!(filtered.links[0].id, l2.id);

        let filtered2 = list(&file, None, Some(dt1))?;
        assert_eq!(filtered2.links.len(), 1);
        assert_eq!(filtered2.links[0].id, l1.id);

        Ok(())
    }

    #[test]
    fn test_link_to_rss_item() {
        let link = sample_link();
        let item = link_to_rss_item(&link);

        assert_eq!(item.title.unwrap(), link.title);
        assert_eq!(item.link.unwrap(), link.url);
        assert_eq!(item.description.unwrap(), link.summary.unwrap().content);
        assert_eq!(item.categories.len(), link.tags.len());
        assert!(item.guid.is_some());
        assert!(item.pub_date.is_some());
    }

    #[test]
    fn test_feed_to_rss_xml_basic() {
        let feed = sample_feed();
        let site_title = "Default Site";
        let site_link = "https://example.com";

        let rss_xml =
            feed_to_rss_xml(&feed, site_title, site_link).expect("Failed to generate RSS XML");

        // Basic checks that XML contains expected values
        assert!(rss_xml.contains("<title>Test Feed</title>"));
        assert!(rss_xml.contains("<link>https://example.com</link>"));
        assert!(rss_xml.contains("Example Post"));
        assert!(rss_xml.contains("This is a summary"));
        assert!(rss_xml.contains("rust"));
        assert!(rss_xml.contains("rss"));
        assert!(rss_xml.contains("urn:uuid:1234"));
    }

    #[test]
    fn test_feed_to_rss_xml_empty_feed_title() {
        let mut feed = sample_feed();
        feed.title = "".to_string();

        let rss_xml = feed_to_rss_xml(&feed, "Default Site", "https://example.com")
            .expect("Failed to generate RSS XML");

        // Should fallback to site title
        assert!(rss_xml.contains("<title>Default Site</title>"));
    }

    #[test]
    fn test_link_without_summary_or_tags() {
        let link = Link {
            id: "5678".to_string(),
            title: "No Summary Post".to_string(),
            url: "https://example.com/nosummary".to_string(),
            via: None,
            summary: None,
            tags: vec![],
            datetime: None,
        };

        let item = link_to_rss_item(&link);

        // description should be None
        assert!(item.description.is_none());
        // categories should be empty
        assert!(item.categories.is_empty());
        // pub_date should be None
        assert!(item.pub_date.is_none());
    }
}
