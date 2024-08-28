use clap::{Arg, Command};

pub struct Args {
    pub rsync_options: String,
    pub src_dir: String,
    pub target_dir: String,
    pub run_size_mb: u64,
    pub enable_parallel: bool,
}

impl Args {
    pub fn parse() -> Self {
        let matches = Command::new("rgigasync")
            .version("1.0")
            .author("Josh Belke <joshbelke@gmail.com>")
            .about("Tool that enables rsync to mirror enormous directory trees")
            .arg(Arg::new("rsync-options").required(true).index(1))
            .arg(Arg::new("src-dir").required(true).index(2))
            .arg(Arg::new("target-dir").required(true).index(3))
            .arg(Arg::new("run-size-mb").default_value("256").index(4))
            .arg(Arg::new("parallel").long("parallel").action(clap::ArgAction::SetTrue))
            .get_matches();

        Args {
            rsync_options: matches.get_one::<String>("rsync-options").unwrap().to_string(),
            src_dir: matches.get_one::<String>("src-dir").unwrap().to_string(),
            target_dir: matches.get_one::<String>("target-dir").unwrap().to_string(),
            run_size_mb: matches.get_one::<String>("run-size-mb").unwrap().parse::<u64>().unwrap() * 1024 * 1024,
            enable_parallel: matches.get_flag("parallel"),
        }
    }
}
