extern crate chrono;
extern crate dirs;
extern crate imap;
extern crate imap_proto;
extern crate maildir;
extern crate native_tls;
extern crate regex;
extern crate rusqlite;
extern crate serde;
extern crate toml;
#[macro_use]
extern crate serde_derive;

use std::thread::spawn;

mod cache;
mod config;
mod imapw;
mod maildirw;
mod syncdir;
use config::Config;
use imapw::Session;
use syncdir::SyncDir;

fn main() {
    let baseconfig = Config::new();
    let config = baseconfig.clone();
    let mut imap_session = Session::new(&config).unwrap();

    let mut threads = vec![];
    match imap_session.list(None, Some("*")) {
        Ok(listing) => {
            for mailbox in listing.iter() {
                /*
                println!(
                    "attributes: {:?} delim: {:?} name: {}",
                    mailbox.attributes(),
                    mailbox.delimiter(),
                    mailbox.name()
                );
                */
                if mailbox.name() == "INBOX"
                    && !mailbox
                        .attributes()
                        .contains(&imap::types::NameAttribute::NoSelect)
                {
                    // select it and sync
                    match SyncDir::new(&baseconfig, mailbox.name().to_string()) {
                        Err(e) => panic!("Sync failed: {}", e),
                        Ok(mut sd) => threads.push(spawn(move || sd.sync())),
                    }
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
