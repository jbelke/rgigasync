#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_option() {
        let matches = Command::new("rgigasync")
            .arg(Arg::new("parallel")
                .long("parallel")
                .help("Enable parallel processing for faster execution")
                .action(clap::ArgAction::SetTrue))
            .get_matches_from(vec!["rgigasync", "--parallel"]);

        assert!(matches.get_flag("parallel"));
    }
}
