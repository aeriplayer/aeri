//! Runs the built `aeri` binary as a user would.

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    #[cfg_attr(miri, ignore = "spawns a process, which Miri cannot do")]
    fn runs_and_exits_successfully() {
        let status = Command::new(env!("CARGO_BIN_EXE_aeri"))
            .status()
            .expect("the aeri binary should start");

        assert!(status.success(), "aeri exited with {status}");
    }
}
