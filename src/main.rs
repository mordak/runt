extern crate dirs;
extern crate imap;
extern crate native_tls;
extern crate regex;
extern crate toml;
#[macro_use]
extern crate serde_derive;

use std::thread::spawn;

mod cachefile;
mod config;
mod syncdir;
use config::Config;
use syncdir::SyncDir;

fn main() {
    let baseconfig = Config::new();
    let config = baseconfig.clone();
    let mut imap_client = config.connect().unwrap();
    imap_client.debug = true;
    println!("logging in");
    let mut imap_session = imap_client
        .login(config.username.as_str(), config.password.unwrap().as_str())
        .unwrap();

    // TODO: get capabilities and bail if no IDLE
    println!("logged in");

    let mut threads = vec![];
    match imap_session.list(None, Some("*")) {
        Ok(listing) => {
            for mailbox in listing.iter() {
                println!(
                    "attributes: {:?} delim: {:?} name: {}",
                    mailbox.attributes(),
                    mailbox.delimiter(),
                    mailbox.name()
                );
                if mailbox.name() == "INBOX"
                    && !mailbox
                        .attributes()
                        .contains(&imap::types::NameAttribute::NoSelect)
                {
                    // select it and IDLE
                    println!("Selecting {}", mailbox.name());
                    let sd = SyncDir::new(&baseconfig, mailbox.name().to_string());
                    threads.push(spawn(move || sd.sync()));
                }
            }
        }
        Err(e) => println!("Error getting listing: {}", e),
    };
    for t in threads {
        t.join().unwrap();
    }

    imap_session.logout().unwrap();
}
