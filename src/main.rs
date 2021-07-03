extern crate anyhow;
extern crate chrono;
extern crate dirs_next;
extern crate imap;
extern crate libc;
extern crate maildir;
extern crate notify;
extern crate regex;
extern crate rusqlite;
extern crate serde;
extern crate toml;
#[macro_use]
extern crate serde_derive;
extern crate rustls_connector;

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
        let mut idle_mailboxes = Vec::new();
        let mut pool_mailboxes = Vec::new();
        match imap.list(None, Some("*")) {
            Ok(listing) => {
                for mailbox in listing.iter() {
                    if !mailbox
                        .attributes()
                        .contains(&imap::types::NameAttribute::NoSelect)
                        && !config.is_mailbox_excluded(mailbox.name())
                    {
                        // select it and sync
                        match SyncDir::new(&config, mailbox.name().to_string()) {
                            Err(e) => panic!("Sync failed: {}", e),
                            Ok(sd) => {
                                notifications.push(sd.sender.clone());
                                if sd.should_idle() {
                                    idle_mailboxes.push(sd);
                                } else {
                                    pool_mailboxes.push(sd);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => println!("Error getting listing: {}", e),
        };
        imap.logout().ok();

        // Handle if the user has specified some maximum number of threads
        // to run with. We have to allocate one thread for every idle
        // mailbox, and remaining threads do all of the sync-once mailboxes.
        let mut pool_size = pool_mailboxes.len();
        if let Some(max_threads) = config.max_concurrency {
            if let Some(pool) = max_threads.checked_sub(idle_mailboxes.len()) {
                pool_size = pool;
            } else {
                pool_size = 0;
            }

            if pool_size == 0 && !pool_mailboxes.is_empty() {
                println!("Account {}.max_concurrency ({}) is too small for the number of idle mailboxes ({}) and non-idle mailboxes.", config.account, max_threads, idle_mailboxes.len(), );
                println!("You may see errors from the server and some mailboxes may not be synchronized.\nTo fix this, specify a number of mailboxes to idle that is smaller that max_concurrency, or increase max_concurrency if possible.");
                pool_size = 1;
            }
        }

        idle_mailboxes.into_iter().for_each(|mut sd| {
            threads.push(spawn(move || sd.sync()));
        });

        if !pool_mailboxes.is_empty() {
            if let Ok(pool) = rayon::ThreadPoolBuilder::new()
                .num_threads(pool_size)
                .build()
            {
                pool_mailboxes.into_iter().for_each(|mut sd| {
                    pool.spawn(move || {
                        if let Err(e) = sd.sync() {
                            eprintln!("Synchronize-once for mailbox {} failed: {}", sd.mailbox, e);
                        }
                    })
                });
            }
        }
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
