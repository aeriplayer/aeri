//! Reading M3U playlist text.

use std::path::Path;

use aeri_core::Source;

/// The sources an M3U or M3U8 playlist lists, in order.
///
/// One path per line. Blank lines and `#` lines (the `#EXTM3U` header, `#EXTINF` and other
/// comments) are skipped, as are lines that name a URL, which aeri cannot play. A relative
/// path is resolved against `base`, the playlist's own directory; surrounding whitespace is
/// ignored. A UTF-8 byte-order mark and CRLF line endings are accepted.
///
/// This reads no files and checks nothing exists: a path that is not there is for the player to
/// report when it comes to it. It never panics, whatever the text.
#[must_use]
pub fn parse_m3u(text: &str, base: &Path) -> Vec<Source> {
    text.strip_prefix('\u{feff}')
        .unwrap_or(text)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !is_url(line))
        .map(|line| Source::File(base.join(line)))
        .collect()
}

/// `scheme://…`, where the scheme is at least two letters, so `C://` is not mistaken for one.
fn is_url(line: &str) -> bool {
    line.split_once("://").is_some_and(|(scheme, _)| {
        scheme.len() >= 2
            && scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use proptest::prelude::*;

    use super::*;

    fn paths(text: &str) -> Vec<PathBuf> {
        parse_m3u(text, Path::new("/music"))
            .into_iter()
            .map(|source| match source {
                Source::File(path) => path,
                _ => panic!("the parser only makes file sources"),
            })
            .collect()
    }

    #[test]
    fn each_line_is_a_path_resolved_against_the_playlists_directory() {
        assert_eq!(
            paths("a.flac\nsub/b.mp3\n"),
            [
                PathBuf::from("/music/a.flac"),
                PathBuf::from("/music/sub/b.mp3")
            ]
        );
    }

    #[test]
    fn an_absolute_path_is_left_alone() {
        assert_eq!(
            paths("/elsewhere/a.flac"),
            [PathBuf::from("/elsewhere/a.flac")]
        );
    }

    #[test]
    fn an_empty_base_leaves_a_relative_path_relative() {
        let sources = parse_m3u("a.flac", Path::new(""));

        assert_eq!(sources.len(), 1);
        assert!(matches!(&sources[0], Source::File(path) if path == Path::new("a.flac")));
    }

    #[test]
    fn comments_blank_lines_and_the_extended_header_are_skipped() {
        let text = "#EXTM3U\n#EXTINF:123,Artist - Title\na.flac\n\n   \n# a comment\nb.flac\n";

        assert_eq!(
            paths(text),
            [
                PathBuf::from("/music/a.flac"),
                PathBuf::from("/music/b.flac")
            ]
        );
    }

    #[test]
    fn crlf_line_endings_and_a_byte_order_mark_are_accepted() {
        assert_eq!(
            paths("\u{feff}#EXTM3U\r\na.flac\r\nb.flac\r\n"),
            [
                PathBuf::from("/music/a.flac"),
                PathBuf::from("/music/b.flac")
            ]
        );
    }

    #[test]
    fn a_byte_order_mark_only_counts_at_the_very_start() {
        assert_eq!(paths("a.flac\n\u{feff}b.flac").len(), 2);
        assert_eq!(
            paths("a.flac\n\u{feff}b.flac")[1],
            PathBuf::from("/music/\u{feff}b.flac")
        );
    }

    #[test]
    fn surrounding_whitespace_is_ignored_but_inner_spaces_are_kept() {
        assert_eq!(paths("  a b.flac \t\n"), [PathBuf::from("/music/a b.flac")]);
    }

    #[test]
    fn a_hash_after_the_first_character_is_part_of_the_name() {
        assert_eq!(
            paths("track #1.flac"),
            [PathBuf::from("/music/track #1.flac")]
        );
    }

    #[test]
    fn urls_are_skipped_because_aeri_cannot_play_them() {
        let text = "http://example.com/a.mp3\nhttps://example.com/b.mp3\nfile:///c.flac\nrtsp://x\nd.flac\n";

        assert_eq!(paths(text), [PathBuf::from("/music/d.flac")]);
    }

    #[test]
    fn a_path_that_merely_contains_a_colon_and_slashes_is_not_a_url() {
        assert_eq!(
            paths("C://music/a.flac\nweird:name//a.flac\nA/b://c.flac"),
            [
                PathBuf::from("/music/C://music/a.flac"),
                PathBuf::from("/music/weird:name//a.flac"),
                PathBuf::from("/music/A/b://c.flac"),
            ]
        );
    }

    #[test]
    fn nothing_in_nothing_out() {
        let nothing: Vec<PathBuf> = Vec::new();
        assert_eq!(paths(""), nothing);
        assert_eq!(paths("\n\r\n"), nothing);
        assert_eq!(paths("\u{feff}"), nothing);
        assert_eq!(paths("#EXTM3U\n#only comments\n"), nothing);
    }

    proptest! {
        #[test]
        fn no_text_makes_the_parser_panic_or_invent_lines(text in any::<String>()) {
            let found = parse_m3u(&text, Path::new("/music"));

            prop_assert!(found.len() <= text.lines().count());
        }

        #[test]
        fn every_listed_name_comes_back_in_order_whatever_the_line_endings(
            names in prop::collection::vec("[a-zA-Z0-9_][a-zA-Z0-9_ ./-]{0,20}[a-zA-Z0-9_]", 0..12),
            crlf in any::<bool>(),
            bom in any::<bool>(),
        ) {
            let end = if crlf { "\r\n" } else { "\n" };
            let mut text = String::new();
            if bom {
                text.push('\u{feff}');
            }
            for name in &names {
                text.push_str(name);
                text.push_str(end);
                text.push_str("# a comment");
                text.push_str(end);
            }

            let expected: Vec<PathBuf> =
                names.iter().map(|name| Path::new("/music").join(name)).collect();
            prop_assert_eq!(paths(&text), expected);
        }

        #[test]
        fn lines_made_only_of_comments_and_whitespace_list_nothing(
            lines in prop::collection::vec("[ \t]*(#[^\r\n]*)?", 0..20),
        ) {
            prop_assert_eq!(paths(&lines.join("\n")), Vec::<PathBuf>::new());
        }
    }
}
