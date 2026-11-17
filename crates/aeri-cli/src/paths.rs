//! Turning what the user typed into the files to play.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use aeri_core::Source;
use aeri_player::{EXTENSIONS, parse_m3u};
use anyhow::{Context as _, Result, bail};

const PLAYLISTS: &[&str] = &["m3u", "m3u8"];

/// The files to play, in the order the user gave them.
///
/// A file is taken as it is, unless it is a playlist, which is read. A directory is searched,
/// including its subdirectories, for files `aeri` can play, which come back sorted by path.
/// Symbolic links to files are followed; symbolic links to directories are not, so a loop
/// cannot trap the search.
///
/// # Errors
///
/// A path that does not exist, a directory with nothing playable in it, or a playlist that
/// lists nothing: each is an error naming the path, so it is reported before any audio opens.
pub(crate) fn expand(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        let metadata =
            fs_err::metadata(input).with_context(|| format!("cannot play {}", input.display()))?;
        let found = if metadata.is_dir() {
            let mut found = Vec::new();
            search(input, &mut found)?;
            found.sort();
            if found.is_empty() {
                bail!("no playable files in {}", input.display());
            }
            found
        } else if has_extension(input, PLAYLISTS) {
            let found = read_playlist(input)?;
            if found.is_empty() {
                bail!("{} lists nothing to play", input.display());
            }
            found
        } else {
            vec![input.clone()]
        };
        files.extend(found);
    }
    Ok(files)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|actual| {
            extensions
                .iter()
                .any(|wanted| actual.eq_ignore_ascii_case(wanted))
        })
}

fn search(directory: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs_err::read_dir(directory)
        .with_context(|| format!("cannot read {}", directory.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", directory.display()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .with_context(|| format!("cannot read {}", path.display()))?;
        if kind.is_dir() {
            search(&path, found)?;
        } else if kind.is_symlink() {
            if fs_err::metadata(&path).is_ok_and(|target| target.is_file())
                && has_extension(&path, EXTENSIONS)
            {
                found.push(path);
            }
        } else if kind.is_file() && has_extension(&path, EXTENSIONS) {
            found.push(path);
        }
    }
    Ok(())
}

fn read_playlist(path: &Path) -> Result<Vec<PathBuf>> {
    let bytes = fs_err::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    Ok(parse_m3u(&text, base)
        .into_iter()
        .filter_map(|source| match source {
            Source::File(file) => Some(file),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A directory under the system's temporary one, removed when it goes out of scope.
    struct Tree(PathBuf);

    impl Tree {
        fn new() -> Self {
            static COUNT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "aeri-cli-test-{}-{}",
                std::process::id(),
                COUNT.fetch_add(1, Ordering::Relaxed)
            ));
            fs_err::create_dir_all(&root).expect("the temporary directory is created");
            Self(root)
        }

        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                fs_err::create_dir_all(parent).expect("the parent is created");
            }
            fs_err::write(&path, contents).expect("the file is written");
            path
        }

        fn dir(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs_err::create_dir_all(&path).expect("the directory is created");
            path
        }

        fn at(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            drop(fs_err::remove_dir_all(&self.0));
        }
    }

    fn expanded(inputs: &[PathBuf]) -> Vec<PathBuf> {
        expand(inputs).expect("the inputs expand")
    }

    fn failure(inputs: &[PathBuf]) -> String {
        format!(
            "{:#}",
            expand(inputs).expect_err("the inputs do not expand")
        )
    }

    #[test]
    fn a_file_is_taken_as_it_is_whatever_its_extension() {
        let tree = Tree::new();
        let odd = tree.file("noise.xyz", b"");
        let plain = tree.file("a.flac", b"");

        assert_eq!(expanded(&[odd.clone(), plain.clone()]), [odd, plain]);
    }

    #[test]
    fn a_directory_gives_its_playable_files_sorted_by_path_at_any_depth() {
        let tree = Tree::new();
        let (a2, a1) = (tree.file("a/02.MP3", b""), tree.file("a/01.flac", b""));
        let deep = tree.file("b/c/03.Flac", b"");
        let top = tree.file("00.flac", b"");
        tree.file("a/cover.jpg", b"");
        tree.file("notes.txt", b"");
        tree.file("list.m3u", b"");

        assert_eq!(expanded(std::slice::from_ref(&tree.0)), [top, a1, a2, deep]);
    }

    #[test]
    fn inputs_keep_the_order_given_and_each_is_expanded_in_place() {
        let tree = Tree::new();
        let later = tree.file("z.flac", b"");
        let one = tree.file("album/1.flac", b"");
        let two = tree.file("album/2.flac", b"");
        let first = tree.file("m.mp3", b"");

        assert_eq!(
            expanded(&[later.clone(), tree.at("album"), first.clone()]),
            [later, one, two, first]
        );
    }

    #[test]
    fn a_playlist_gives_the_paths_it_lists_resolved_against_its_own_directory() {
        let tree = Tree::new();
        let list = tree.file(
            "lists/mix.M3U",
            b"\xef\xbb\xbf#EXTM3U\r\none.flac\r\n../two.mp3\r\nhttp://example.com/x.mp3\r\n",
        );

        assert_eq!(
            expanded(&[list]),
            [
                tree.at("lists").join("one.flac"),
                tree.at("lists").join("../two.mp3"),
            ]
        );
    }

    #[test]
    fn a_playlist_that_is_not_valid_utf8_still_gives_the_names_that_are() {
        let tree = Tree::new();
        let list = tree.file("mix.m3u8", b"se\xf1or.flac\nok.flac\n");

        let found = expanded(&[list]);

        assert_eq!(found.len(), 2);
        assert_eq!(found[1], tree.at("ok.flac"));
    }

    #[test]
    fn a_path_that_does_not_exist_is_an_error_naming_it() {
        let tree = Tree::new();
        let missing = tree.at("gone.flac");

        let message = failure(&[tree.file("a.flac", b""), missing.clone()]);

        assert!(message.contains("cannot play"), "{message}");
        assert!(
            message.contains(&missing.display().to_string()),
            "{message}"
        );
    }

    #[test]
    fn a_directory_with_nothing_playable_is_an_error_naming_it() {
        let tree = Tree::new();
        let empty = tree.dir("empty");
        let texts = tree.dir("texts");
        tree.file("texts/readme.txt", b"");
        tree.file("texts/nested/cover.jpg", b"");

        for directory in [empty, texts] {
            let message = failure(std::slice::from_ref(&directory));
            assert!(message.contains("no playable files in"), "{message}");
            assert!(
                message.contains(&directory.display().to_string()),
                "{message}"
            );
        }
    }

    #[test]
    fn a_playlist_that_lists_nothing_is_an_error_naming_it() {
        let tree = Tree::new();
        let list = tree.file("none.m3u", b"#EXTM3U\n# nothing\n\n");

        let message = failure(std::slice::from_ref(&list));

        assert!(message.contains("lists nothing to play"), "{message}");
        assert!(message.contains(&list.display().to_string()), "{message}");
    }

    #[test]
    fn an_empty_directory_among_good_inputs_is_still_an_error() {
        let tree = Tree::new();
        let good = tree.file("a.flac", b"");

        let message = failure(&[good, tree.dir("empty")]);

        assert!(message.contains("no playable files in"), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn links_to_files_are_followed_and_links_to_directories_and_to_nothing_are_not() {
        use fs_err::os::unix::fs::symlink;

        let tree = Tree::new();
        let real = tree.file("real/song.flac", b"");
        let search = tree.dir("search");
        let text = tree.file("real/notes.txt", b"");
        symlink(&real, search.join("linked.flac")).expect("the link is made");
        symlink(&text, search.join("linked.txt")).expect("the link is made");
        symlink(tree.at("real"), search.join("looped")).expect("the link is made");
        symlink(tree.at("nowhere.flac"), search.join("broken.flac")).expect("the link is made");
        symlink(&search, search.join("self")).expect("the link is made");

        assert_eq!(
            expanded(std::slice::from_ref(&search)),
            [search.join("linked.flac")]
        );
    }
}
