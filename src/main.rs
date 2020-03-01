extern crate chrono;
extern crate dirs;
extern crate imap;
extern crate imap_proto;
extern crate libc;
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
use libc::SIGINT;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use syncdir::SyncDir;

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn main() {
    let baseconfig = Config::new();
    let config = baseconfig.clone();
    let mut imap_session = Session::new(&config).unwrap();

    let shutdown = Arc::new(&SHUTDOWN);
    unsafe {
        libc::signal(SIGINT, handle_sigint as usize);
    }

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
                        Ok(mut sd) => {
                            let stop = shutdown.clone();
                            threads.push(spawn(move || sd.sync(stop)));
                        }
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

#[allow(dead_code)]
fn handle_sigint(_signal: i32) {
    println!("Got SIGINT");
    SHUTDOWN.store(true, Ordering::Relaxed);
}
