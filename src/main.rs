extern crate anyhow;
extern crate chrono;
extern crate dirs;
extern crate imap;
extern crate imap_proto;
extern crate libc;
extern crate maildir;
extern crate native_tls;
extern crate notify;
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
use imapw::Imap;
use libc::SIGINT;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{sleep, spawn};
use std::time;
use syncdir::{SyncDir, SyncMessage};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn main() {
    // set up signal handler for Ctrl-C
    unsafe {
        libc::signal(SIGINT, handle_sigint as usize);
    }

    let mut threads = vec![];
    let mut notifications = vec![];

    // Parse out config and set up sync jobs
    let configs = Config::new();
    for config in configs.accounts {
        let mut imap = Imap::new(&config).unwrap();
        match imap.list(None, Some("*")) {
            Ok(listing) => {
                for mailbox in listing.iter() {
                    if !mailbox
                        .attributes()
                        .contains(&imap::types::NameAttribute::NoSelect)
                    {
                        // select it and sync
                        match SyncDir::new(&config, mailbox.name().to_string()) {
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
        imap.logout().ok();
    }

    // spin off the thread to wait for Ctrl-C
    threads.push(spawn(move || {
        while !SHUTDOWN.load(Ordering::Relaxed) {
            sleep(time::Duration::from_millis(1000));
        }
        for s in notifications {
            s.send(SyncMessage::Exit).ok();
        }
        Ok(())
    }));

    for t in threads {
        if let Err(what) = t.join().unwrap() {
            eprintln!("Error joining sync thread: {}", what);
        }
    }
}

#[allow(dead_code)]
fn handle_sigint(_signal: i32) {
    println!("Shutting down...");
    SHUTDOWN.store(true, Ordering::Relaxed);
}
