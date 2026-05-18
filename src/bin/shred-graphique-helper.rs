use std::process::{Command, exit};
use std::io::Write;
use std::os::unix::process::CommandExt;

const SHRED_PATH: &str = "/usr/bin/shred";
const KILL_PATH: &str = "/usr/bin/kill";
const SMARTCTL_PATH: &str = "/usr/sbin/smartctl";
const BLKID_PATH: &str = "/usr/bin/blkid";
const TRUE_PATH: &str = "/usr/bin/true";

fn run_command(path: &str, args: impl Iterator<Item = String>) -> i32 {
    let args: Vec<String> = args.collect();
    match Command::new(path).args(&args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 1,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let op = args.next().unwrap_or_else(|| {
        eprintln!("Usage: shred-graphique-helper <shred|kill|smartctl|blkid|true> [args...]");
        exit(2);
    });

    let code = match op.as_str() {
        "shred" => {
            println!("PID:{}", std::process::id());
            let _ = std::io::stdout().flush();
            let err = Command::new(SHRED_PATH).args(args).exec();
            eprintln!("exec failed: {}", err);
            1
        }
        "kill" => run_command(KILL_PATH, args),
        "smartctl" => run_command(SMARTCTL_PATH, args),
        "blkid" => run_command(BLKID_PATH, args),
        "true" => run_command(TRUE_PATH, args),
        _ => {
            eprintln!("Commande non autorisée: {}", op);
            2
        }
    };

    exit(code);
}
