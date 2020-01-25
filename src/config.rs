use native_tls::{Certificate};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub account: String,
    pub server: String,
    pub port: Option<u16>,
    pub server_ca_path: Option<String>,
    pub username: String,
    pub maildir: String,
    pub password_command: Option<String>,
    pub password: Option<String>,
}

impl Config {
    pub fn new() -> Config {
        let mut dir = Config::dir();
        dir.push("config");
        let mut f = File::open(dir).unwrap();
        let mut buf: String = String::new();
        f.read_to_string(&mut buf).unwrap();
        let mut config: Config = toml::from_str(&buf).unwrap();
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
        config
    }

    pub fn dir() -> PathBuf {
        let mut home = match dirs::home_dir() {
            Some(path) => path,
            _ => PathBuf::from(""),
        };
        home.push(".runt");
        home
    }

    pub fn get_server_ca_cert(&self) -> Option<Certificate> {
        if let Some(ca_path) = &self.server_ca_path {
            let mut certbuf: Vec<u8> = Vec::new();
            let mut certfile = File::open(ca_path).unwrap();
            certfile.read_to_end(&mut certbuf).unwrap();
            return Some(Certificate::from_pem(&certbuf).unwrap());
        }
        None
    }
}
