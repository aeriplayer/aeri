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
    #[cfg_attr(miri, ignore = "spawns a process, which Miri cannot do")]
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
    #[cfg_attr(miri, ignore = "spawns a process, which Miri cannot do")]
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
    #[cfg_attr(miri, ignore = "spawns a process, which Miri cannot do")]
    fn play_without_a_path_is_a_usage_error() {
        let output = aeri(&["play"]);

        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage"));
    }
}
