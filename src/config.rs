use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
use std::vec::Vec;

#[derive(Deserialize, Clone)]
pub struct Account {
    pub account: String,
    pub server: String,
    pub port: Option<u16>,
    pub username: String,
    pub maildir: String,
    pub password_command: Option<String>,
    pub password: Option<String>,
    pub exclude: Option<Vec<String>>,
    pub idle: Option<Vec<String>>,
    pub max_concurrency: Option<usize>,
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub accounts: Vec<Account>,
}

impl Config {
    pub fn new() -> Config {
        let mut dir = Config::dir();
        dir.push("config");
        let mut f = File::open(dir).unwrap();
        let mut buf: String = String::new();
        f.read_to_string(&mut buf).unwrap();
        let mut configs: Config = toml::from_str(&buf).unwrap();
        for config in &mut configs.accounts {
            if config.port.is_none() {
                config.port = Some(993);
            }
            if config.password_command.is_some() {
                let password = Command::new("sh")
                    .arg("-c")
                    .arg(config.password_command.clone().unwrap())
                    .output()
                    .expect("Could not execute password_command");
                config.password = Some(
                    String::from_utf8(password.stdout.as_slice().to_vec())
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
        }
        configs
    }

    pub fn dir() -> PathBuf {
        let mut home = match dirs_next::home_dir() {
            Some(path) => path,
            _ => PathBuf::from(""),
        };
        home.push(".runt");
        home
    }
}

impl Account {
    /// Is this mailbox excluded from synchronization?
    pub fn is_mailbox_excluded(&self, name: &str) -> bool {
        if let Some(exclude) = &self.exclude {
            exclude.contains(&name.to_string())
        } else {
            false
        }
    }

    /// Is this mailbox one we want to IDLE on?
    /// If the account has a `idle` member, then only mailboxes
    /// in that list are IDLEd. Otherwise everything that is not
    /// `exclude`d is IDLEd.
    pub fn is_mailbox_idled(&self, name: &str) -> bool {
        if let Some(idle) = &self.idle {
            idle.contains(&name.to_string())
        } else {
            true
        }
    }
}
