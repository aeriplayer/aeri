//! Runs the built `aeri` binary as a user would.

#[cfg(test)]
mod tests {
    use std::process::{Command, Output};

    fn aeri(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_aeri"))
            .args(args)
            .output()
            .expect("the aeri binary starts")
    }

    #[test]
    fn help_lists_the_play_command() {
        let output = aeri(&["--help"]);

        assert!(
            output.status.success(),
            "aeri --help exited with {}",
            output.status
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("play"));
    }

    #[test]
    fn playing_a_missing_file_fails_and_names_it() {
        let output = aeri(&["play", "no-such-file.flac"]);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "aeri exited successfully");
        assert!(stderr.starts_with("Error:"), "stderr was {stderr:?}");
        assert!(
            stderr.contains("no-such-file.flac"),
            "stderr was {stderr:?}"
        );
    }

    #[test]
    fn play_without_a_path_is_a_usage_error() {
        let output = aeri(&["play"]);

        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage"));
    }

    /// A directory under the system's temporary one, removed when it goes out of scope.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("aeri-cli-it-{}-{name}", std::process::id()));
            fs_err::create_dir_all(&path).expect("the directory is created");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs_err::remove_dir_all(&self.0));
        }
    }

    fn fails_naming(args: &[&str], needle: &str) {
        let output = aeri(args);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "aeri exited successfully");
        assert!(stderr.starts_with("Error:"), "stderr was {stderr:?}");
        assert!(stderr.contains(needle), "stderr was {stderr:?}");
    }

    #[test]
    fn a_directory_with_nothing_playable_fails_and_names_it() {
        let scratch = Scratch::new("empty-dir");
        fs_err::write(scratch.0.join("readme.txt"), "not audio").expect("the file is written");
        let dir = scratch.0.display().to_string();

        fails_naming(&["play", &dir], "no playable files in");
        fails_naming(&["play", &dir], &dir);
    }

    #[test]
    fn a_playlist_that_lists_nothing_fails_and_names_it() {
        let scratch = Scratch::new("empty-list");
        let list = scratch.0.join("none.m3u");
        fs_err::write(&list, "#EXTM3U\n# nothing\n").expect("the file is written");
        let list = list.display().to_string();

        fails_naming(&["play", &list], "lists nothing to play");
        fails_naming(&["play", &list], &list);
    }

    #[test]
    fn one_missing_path_among_good_ones_fails_before_anything_plays() {
        let scratch = Scratch::new("mixed");
        let good = scratch.0.join("a.flac");
        fs_err::write(&good, "").expect("the file is written");

        fails_naming(
            &["play", &good.display().to_string(), "no-such-file.flac"],
            "no-such-file.flac",
        );
    }

    #[test]
    fn a_volume_that_is_not_a_number_is_a_usage_error() {
        let output = aeri(&["play", "--volume", "loud", "a.flac"]);

        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("decibels"));
    }

    #[test]
    fn help_lists_the_play_flags() {
        let output = aeri(&["play", "--help"]);
        let stdout = String::from_utf8_lossy(&output.stdout);

        for flag in [
            "--order",
            "--volume",
            "--replaygain",
            "shuffle",
            "repeat-playlist",
        ] {
            assert!(stdout.contains(flag), "missing {flag} in {stdout}");
        }
    }
}
