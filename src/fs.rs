use crate::linkleaf_proto::Feed;
use anyhow::{Context, Result};
use prost::Message;
use std::path::Path;
use std::{fs, io::Write};

/// Read a protobuf feed from disk.
///
/// ## Behavior
/// - Reads the entire file at `path` into memory.
/// - Decodes the bytes into a [`Feed`] using `prost`’s `Message::decode`.
///
/// ## Arguments
/// - `path`: Path to the `.pb` file to read.
///
/// ## Returns
/// The decoded [`Feed`] on success.
///
/// ## Errors
/// - I/O errors from [`fs::read`], wrapped with context
///   `"failed to read {path}"`.
/// - Protobuf decode errors from `Feed::decode`, wrapped with context
///   `"failed to decode protobuf: {path}"`.
/// - The error type is [`anyhow::Error`] via your crate-wide `Result`.
///
/// ## Example
/// ```no_run
/// use std::path::PathBuf;
/// use linkleaf_core::fs::read_feed;
/// use anyhow::Result;
///
/// fn main() -> Result<()> {
///     let path = PathBuf::from("mylinks.pb");
///     let feed = read_feed(&path)?;
///     println!("title: {}, links: {}", feed.title, feed.links.len());
///     Ok::<(), anyhow::Error>(())
/// }
/// ```
pub fn read_feed<P: AsRef<Path>>(path: P) -> Result<Feed> {
    let path = path.as_ref();
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Feed::decode(bytes.as_slice())
        .with_context(|| format!("failed to decode protobuf: {}", path.display()))
}

/// Write a protobuf feed to disk **atomically** (best-effort).
///
/// ## Behavior
/// - Ensures the parent directory of `path` exists (creates it if needed).
/// - Encodes `feed` to a temporary file with extension `".pb.tmp"`.
/// - Flushes and then renames the temp file over `path`.
///   - On Unix/POSIX, the rename is atomic when source and destination are on
///     the same filesystem.
///   - On Windows, `rename` may fail if the destination exists; this function
///     forwards that error as-is.
///
/// The input `feed` is consumed and returned unchanged on success to make
/// call sites ergonomic.
///
/// ## Arguments
/// - `path`: Destination path of the `.pb` file.
/// - `feed`: The feed to persist (consumed).
///
/// ## Returns
/// The same [`Feed`] value that was written (handy for chaining).
///
/// ## Errors
/// - Directory creation errors from [`fs::create_dir_all`], with context
///   `"failed to create directory {dir}"`.
/// - File creation/write/flush errors for the temporary file, with context
///   `"failed to write {tmp}"`.
/// - Rename errors when moving the temp file into place, with context
///   `"failed to move temp file into place: {path}"`.
/// - Protobuf encode errors from `feed.encode(&mut buf)`.
/// - The error type is [`anyhow::Error`] via your crate-wide `Result`.
///
/// ## Example
/// ```no_run
/// use std::path::PathBuf;
/// use linkleaf_core::fs::{read_feed, write_feed};
/// use anyhow::Result;
///
/// fn main() -> Result<()> {
///     let path = PathBuf::from("mylinks.pb");
///     let mut feed = read_feed(&path)?;        // or Feed { .. } if creating anew
///     feed.title = "My Links".into();
///     let written = write_feed(&path, feed)?;  // atomic write
///     assert_eq!(written.title, "My Links");
///     Ok(())
/// }
/// ```
///
/// ## Notes
/// - Atomicity requires the temporary file and the destination to be on the
///   **same filesystem**.
/// - If multiple processes may write concurrently, consider adding a file lock
///   around the write section.
pub fn write_feed<P: AsRef<Path>>(path: P, feed: Feed) -> Result<Feed> {
    let path = path.as_ref();
    // Ensure parent directory exists (if any)
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create directory {}", dir.display()))?;
    }

    let mut buf = Vec::with_capacity(1024);
    feed.encode(&mut buf)
        .context("failed to encode protobuf Feed")?;

    let tmp = path.with_extension("pb.tmp");
    {
        let mut f =
            fs::File::create(&tmp).with_context(|| format!("failed to write {}", tmp.display()))?;
        f.write_all(&buf)?;
        // Ensure bytes are on disk, not just in the OS page cache
        f.sync_all()?;
    }
    fs::rename(&tmp, &path)
        .with_context(|| format!("failed to move temp file into place: {}", path.display()))?;
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::{read_feed, write_feed};
    use crate::linkleaf_proto::Feed;
    use anyhow::Result;
    use std::{fs, path::PathBuf};
    use tempfile::tempdir;

    // Small helper to build a Feed with just the fields we care about.
    // Prost-generated types usually derive Default + Clone + PartialEq.
    fn mk_feed(title: &str) -> Feed {
        Feed {
            title: title.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn write_then_read_roundtrip() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("feed.pb");

        let original = mk_feed("Roundtrip");
        let written = write_feed(&path, original.clone())?;
        // write_feed returns the same Feed it was given
        assert_eq!(written, original);

        let read = read_feed(&path)?;
        assert_eq!(read, original);

        Ok(())
    }

    #[test]
    fn write_feed_creates_parent_dirs() -> Result<()> {
        let dir = tempdir()?;
        // nested dirs that don't exist yet
        let path: PathBuf = dir.path().join("nested/dir/structure/feed.pb");

        let feed = mk_feed("Nested OK");
        write_feed(&path, feed)?;

        assert!(path.exists(), "destination file should exist");
        Ok(())
    }

    #[test]
    fn write_feed_overwrites_existing_and_no_tmp_left() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("feed.pb");
        let tmp = path.with_extension("pb.tmp");

        let first = mk_feed("v1");
        write_feed(&path, first)?;

        let second = mk_feed("v2");
        write_feed(&path, second.clone())?;

        let read_back = read_feed(&path)?;
        assert_eq!(read_back.title, "v2");
        assert!(
            !tmp.exists(),
            "temporary file should not remain after successful rename"
        );
        Ok(())
    }

    #[test]
    fn read_feed_nonexistent_file_errors_with_context() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does_not_exist.pb");

        let err = read_feed(&path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failed to read"),
            "error should contain read context, got: {msg}"
        );
    }

    #[test]
    fn read_feed_invalid_protobuf_errors_with_context() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("invalid.pb");

        // Write junk bytes so prost::Message::decode fails
        fs::write(&path, b"this is not a protobuf")?;

        let err = read_feed(&path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failed to decode protobuf:"),
            "error should contain decode context, got: {msg}"
        );
        Ok(())
    }
}
