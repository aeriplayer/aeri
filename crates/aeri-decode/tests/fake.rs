//! The in-memory fake stays true to the committed fixtures it stands in for (ADR 0029).

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aeri_core::Source;
    use aeri_decode::{DecodeError, fake_open, open};

    fn fixture(name: &str) -> Source {
        let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
            .iter()
            .collect();
        Source::File(path)
    }

    #[test]
    fn every_fixture_reports_what_its_fake_reports() {
        for name in [
            "sine.flac",
            "sine.mp3",
            "sine-48k.flac",
            "sine-rg.flac",
            "sine-rg.mp3",
            "sine-rg-bad.flac",
        ] {
            let real = open(&fixture(name)).expect("a fixture opens");
            let fake = fake_open(&fixture(name)).expect("a fixture opens");
            assert_eq!(fake.spec(), real.spec(), "{name}: spec");
            assert_eq!(fake.duration(), real.duration(), "{name}: duration");
            assert_eq!(
                fake.replay_gain(),
                real.replay_gain(),
                "{name}: replay gain"
            );
        }
    }

    #[test]
    fn the_text_file_fails_the_same_way_in_both() {
        let real = open(&fixture("not-audio.flac")).err().expect("it fails");
        let fake = fake_open(&fixture("not-audio.flac"))
            .err()
            .expect("it fails");
        assert!(matches!(real, DecodeError::Unsupported(_)));
        assert!(matches!(fake, DecodeError::Unsupported(_)));
    }

    #[test]
    fn a_missing_file_fails_the_same_way_in_both() {
        let real = open(&fixture("missing.flac")).err().expect("it fails");
        let fake = fake_open(&fixture("missing.flac")).err().expect("it fails");
        let kind = |error: &DecodeError| match error {
            DecodeError::Io(io) => Some(io.kind()),
            _ => None,
        };
        assert_eq!(kind(&real), kind(&fake));
        assert!(kind(&fake).is_some());
    }

    #[test]
    fn every_fixture_in_the_directory_is_one_the_fake_knows() {
        let dir: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures"]
            .iter()
            .collect();
        for entry in fs_err::read_dir(dir).expect("the directory exists") {
            let entry = entry.expect("an entry");
            let name = entry.file_name().into_string().expect("utf-8 names");
            let extension = entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_owned);
            if extension.is_some_and(|e| aeri_decode::EXTENSIONS.contains(&e.as_str())) {
                let known = matches!(
                    fake_open(&fixture(&name)),
                    Ok(_) | Err(DecodeError::Unsupported(_))
                );
                assert!(known, "{name} has no fake; add it to fake_open");
            }
        }
    }
}
