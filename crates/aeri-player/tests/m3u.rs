//! Parses the committed M3U fixtures, the way the CLI reads a playlist file.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use aeri_core::Source;
    use aeri_player::parse_m3u;

    const BASE: &str = "/library";

    fn read(name: &str) -> String {
        let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
            .iter()
            .collect();
        let bytes = fs_err::read(path).expect("the fixture exists");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn parsed(name: &str) -> Vec<PathBuf> {
        parse_m3u(&read(name), Path::new(BASE))
            .into_iter()
            .map(|source| match source {
                Source::File(path) => path,
                _ => panic!("the parser only makes file sources"),
            })
            .collect()
    }

    fn under(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| Path::new(BASE).join(name))
            .collect()
    }

    #[test]
    fn a_plain_list_gives_its_paths_resolved_against_the_base() {
        assert_eq!(
            parsed("basic.m3u"),
            [
                PathBuf::from("/library/one.flac"),
                PathBuf::from("/library/album/two.mp3"),
                PathBuf::from("/absolute/three.flac"),
            ]
        );
    }

    #[test]
    fn the_extended_format_gives_the_same_paths_without_its_comments() {
        assert_eq!(
            parsed("extended.m3u"),
            under(&["one.flac", "album/two.mp3"])
        );
    }

    #[test]
    fn non_ascii_names_are_read_correctly() {
        assert_eq!(parsed("unicode.m3u"), under(&["café.flac", "東京/two.mp3"]));
    }

    #[test]
    fn a_messy_list_gives_only_its_real_entries() {
        assert_eq!(
            parsed("messy.m3u"),
            under(&["indented.flac", "with space/and #hash.flac"])
        );
    }

    #[test]
    fn a_list_of_comments_gives_nothing() {
        assert_eq!(parsed("empty.m3u"), Vec::<PathBuf>::new());
    }
}
