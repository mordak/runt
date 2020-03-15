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

mod cache;
mod config;
mod imapw;
mod maildirw;
mod syncdir;
use config::Config;
use imapw::Session;
use libc::SIGINT;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{sleep, spawn};
use std::time;
use syncdir::{SyncDir, SyncMessage};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn main() {
    let baseconfig = Config::new();
    let config = baseconfig.clone();
    let mut imap_session = Session::new(&config).unwrap();

    unsafe {
        libc::signal(SIGINT, handle_sigint as usize);
    }

    let mut threads = vec![];
    let mut notifications = vec![];

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
                if mailbox.name() == "admin"
                    && !mailbox
                        .attributes()
                        .contains(&imap::types::NameAttribute::NoSelect)
                {
                    // select it and sync
                    match SyncDir::new(&baseconfig, mailbox.name().to_string()) {
                        Err(e) => panic!("Sync failed: {}", e),
                        Ok(mut sd) => {
                            notifications.push(sd.sender.clone());
                            threads.push(spawn(move || sd.sync()));
                        }
                    }
                }
            }
        }
        Err(e) => println!("Error getting listing: {}", e),
    };

    // spin off the thread to wait for Ctrl-C
    threads.push(spawn(move || {
        while !SHUTDOWN.load(Ordering::Relaxed) {
            sleep(time::Duration::from_millis(1000));
        }
        for s in notifications {
            s.send(SyncMessage::Exit).ok();
        }
    }));

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
