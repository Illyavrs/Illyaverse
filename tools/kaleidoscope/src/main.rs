use kaleidoscope::{
    default_hosts_path, managed_range, plan_apply, plan_restore, read_hosts, stable_domains,
    validate_domain, write_plan, Mapping, Result,
};
use std::path::PathBuf;

const HELP: &str = "Kaleidoscope — Illyaverse connection assistant\n\n\
Usage:\n\
  kaleidoscope inspect [--hosts PATH]\n\
  kaleidoscope plan --ip IP --domain DOMAIN [--domain DOMAIN...] [--hosts PATH]\n\
  kaleidoscope apply --ip IP --domain DOMAIN [--domain DOMAIN...] [--hosts PATH] [--write | --dry-run]\n\
  kaleidoscope restore [--hosts PATH] [--write | --dry-run]\n\
  kaleidoscope profile --client stable|lazer --domain DOMAIN\n\n\
All commands preview by default. Only apply/restore with --write modify a file.\n\
No certificate installation, DNS cache changes, or client launches are performed.\n\
System hosts writes need administrator/root permissions. Close other hosts editors first.\n\
See docs/connection.md for TLS and client compatibility requirements.";

#[derive(Debug)]
struct Args {
    command: String,
    hosts: PathBuf,
    ip: Option<String>,
    domains: Vec<String>,
    client: Option<String>,
    write: bool,
    dry_run: bool,
}

fn parse_args(values: Vec<String>) -> Result<Args> {
    let mut values = values.into_iter();
    let command = values.next().unwrap_or_else(|| "help".into());
    if ![
        "inspect",
        "plan",
        "apply",
        "restore",
        "profile",
        "help",
        "--help",
        "-h",
        "--version",
    ]
    .contains(&command.as_str())
    {
        return Err(format!(
            "Unknown command: {command}. Run kaleidoscope --help"
        ));
    }
    let mut args = Args {
        command,
        hosts: default_hosts_path(),
        ip: None,
        domains: vec![],
        client: None,
        write: false,
        dry_run: false,
    };
    let mut hosts_given = false;
    while let Some(flag) = values.next() {
        match flag.as_str() {
            "--write" if !args.write => args.write = true,
            "--dry-run" if !args.dry_run => args.dry_run = true,
            "--hosts" if !hosts_given => {
                args.hosts = PathBuf::from(values.next().ok_or("--hosts needs a path")?);
                hosts_given = true;
            }
            "--ip" if args.ip.is_none() => {
                args.ip = Some(values.next().ok_or("--ip needs an address")?)
            }
            "--domain" => args
                .domains
                .push(values.next().ok_or("--domain needs a hostname")?),
            "--client" if args.client.is_none() => {
                args.client = Some(values.next().ok_or("--client needs stable or lazer")?)
            }
            _ => return Err(format!("Unknown or duplicate option: {flag}")),
        }
    }
    if args.write && args.dry_run {
        return Err("--write and --dry-run cannot be used together".into());
    }
    if (args.write || args.dry_run) && !["apply", "restore"].contains(&args.command.as_str()) {
        return Err("--write/--dry-run are only accepted by apply and restore".into());
    }
    if args.client.is_some() && args.command != "profile" {
        return Err("--client is only accepted by profile".into());
    }
    if args.command == "profile" {
        if hosts_given || args.ip.is_some() {
            return Err("profile does not accept --hosts or --ip".into());
        }
        if args.domains.len() != 1 {
            return Err("profile requires exactly one --domain, the server root domain".into());
        }
        if !matches!(args.client.as_deref(), Some("stable" | "lazer")) {
            return Err("profile requires --client stable or lazer".into());
        }
    } else if !["plan", "apply"].contains(&args.command.as_str())
        && (args.ip.is_some() || !args.domains.is_empty())
    {
        return Err("--ip/--domain are only accepted by plan, apply, or profile".into());
    }
    Ok(args)
}

fn run(args: Args) -> Result<()> {
    match args.command.as_str() {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            return Ok(());
        }
        "--version" => {
            println!("kaleidoscope {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        "profile" => {
            let domain = validate_domain(&args.domains[0])?;
            if args.client.as_deref() == Some("stable") {
                println!("Stable launch arguments (preview):\n-devserver {domain}\n\nPowerShell shortcut example:\n& 'C:\\Games\\osu!\\osu!.exe' '-devserver' '{domain}'\n\nOperator-owned DNS/TLS hosts to configure:");
                for host in stable_domains(&domain)? {
                    println!("{host}");
                }
                println!("\nClient version and server routes still need a real login/score/replay test. See docs/connection.md.");
            } else {
                for prefix in ["osu", "spectator", "bss"] {
                    validate_domain(&format!("{prefix}.{domain}"))?;
                }
                println!("{{\n  \"client\": \"lazer\",\n  \"status\": \"source-build-required; stock-client-unverified\",\n  \"website_url\": \"https://osu.{domain}\",\n  \"api_url\": \"https://osu.{domain}\",\n  \"spectator_url\": \"https://spectator.{domain}/spectator\",\n  \"multiplayer_url\": \"https://spectator.{domain}/multiplayer\",\n  \"metadata_url\": \"https://spectator.{domain}/metadata\",\n  \"beatmap_submission_service_url\": \"https://bss.{domain}\",\n  \"oauth_client_id\": \"operator-must-configure\",\n  \"oauth_client_secret\": \"operator-must-configure\"\n}}");
                eprintln!("Endpoint planning manifest only. Official lazer has no verified arbitrary --server switch. This file is not an importable stock-client profile.");
            }
            return Ok(());
        }
        _ => {}
    }
    let original = read_hosts(&args.hosts)?;
    if args.command == "inspect" {
        println!(
            "Hosts: {}\nEncoding: UTF-8/ASCII\nBytes: {}",
            args.hosts.display(),
            original.len()
        );
        match managed_range(&original)? {
            Some(range) => {
                println!("Kaleidoscope block: present\n{}", &original[range]);
            }
            None => println!("Kaleidoscope block: absent"),
        }
        return Ok(());
    }
    let replacement = if args.command == "restore" {
        plan_restore(&original)?
    } else {
        let mapping = Mapping::new(args.ip.as_deref().ok_or("--ip is required")?, &args.domains)?;
        plan_apply(&original, &mapping)?
    };
    println!("Hosts: {}\nMode: {}\nChanged: {}\n--- proposed hosts contents ---\n{}\n--- end preview ---",
        args.hosts.display(), if args.write { "write" } else { "preview" }, replacement != original, replacement);
    if args.write {
        match write_plan(&args.hosts, &original, &replacement)? {
            Some(backup) => println!("Applied and verified. Byte backup: {}", backup.display()),
            None => println!("Already matches the plan; no files written."),
        }
    } else {
        println!("No file changed. Review the preview, then use --write to apply this command.");
    }
    Ok(())
}

fn main() {
    if let Err(error) = parse_args(std::env::args().skip(1).collect()).and_then(run) {
        eprintln!("Kaleidoscope: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Result<Args> {
        parse_args(values.iter().map(|value| value.to_string()).collect())
    }
    #[test]
    fn writing_is_explicit() {
        assert!(
            !args(&["apply", "--ip", "127.0.0.1", "--domain", "illya.test"])
                .unwrap()
                .write
        );
        assert!(args(&["apply", "--write"]).unwrap().write);
        assert!(args(&["restore", "--dry-run"]).unwrap().dry_run);
        for values in [
            &["apply", "--write", "--dry-run"][..],
            &["plan", "--write"],
            &["inspect", "--write"],
            &[
                "profile",
                "--client",
                "lazer",
                "--domain",
                "illya.test",
                "--write",
            ],
            &["restore", "--ip", "127.0.0.1"],
            &["apply", "--hosts", "a", "--hosts", "b"],
        ] {
            assert!(args(values).is_err());
        }
    }
    #[test]
    fn no_arguments_are_read_only_help() {
        assert_eq!(args(&[]).unwrap().command, "help");
        assert!(args(&["profile", "--client", "lazer", "--domain", "illya.test"]).is_ok());
        assert!(args(&[
            "profile",
            "--client",
            "lazer",
            "--domain",
            "illya.test",
            "--ip",
            "::1"
        ])
        .is_err());
        assert!(args(&["apply", "--domain"]).is_err());
        assert!(args(&["wat"]).is_err());
    }
}
