mod messagemeta;
mod statefile;
mod syncflags;

use self::syncflags::SyncFlags;
use config::Config;
use imap::types::{Flag, Fetch, Mailbox, Uid};
use std::collections::HashSet;
use std::path::PathBuf;

use self::statefile::StateFile;
pub use self::messagemeta::MessageMeta;

// FIXME: Move this to imapw?
/// Convert imap flags to maildir flags
pub fn maildir_flags_from_imap(inflags: &[Flag]) -> String {
    let syncflags = SyncFlags::from(inflags);
    syncflags.to_string()
}

/// Path to the cache directory for given account and mailbox
fn path(account: &str, mailbox: &str) -> PathBuf {
    let mut cachefile = Config::dir();
    cachefile.push("cache");
    cachefile.push(account);
    cachefile.push(mailbox);
    // Create the cache path if it doesn't exist
    std::fs::create_dir_all(&cachefile).ok();
    cachefile
}

/// Path to .state file for given account and mailbox
fn statefile(account: &str, mailbox: &str) -> PathBuf {
    let mut cachefile = self::path(account, mailbox);
    cachefile.push(".state");
    cachefile
}

pub struct Cache {
    dirpath: PathBuf,
    statefile: PathBuf,
    state: StateFile,
}

impl Cache {
    pub fn new(account: &str, mailbox: &str) -> Result<Cache, String> {
        StateFile::new(&self::statefile(account, mailbox)).map(|statefile| Cache {
            dirpath: self::path(account, mailbox),
            statefile: self::statefile(account, mailbox),
            state: statefile,
        })
    }

    pub fn is_valid(&self, mailbox: &Mailbox) -> bool {
        self.state.uid_validity == mailbox.uid_validity.expect("No UIDVALIDITY in Mailbox")
    }

    fn uidfile(&self, uid: Uid) -> PathBuf {
        let mut uidpath = self.dirpath.clone();
        uidpath.push(format!("{}", uid));
        uidpath
    }

    pub fn update_remote_state(&mut self, mailbox: &Mailbox) -> Result<(), String> {
        self.state.remote_last = chrono::offset::Utc::now().timestamp_millis();
        self.state.uid_validity = mailbox.uid_validity.expect("No UIDVALIDITY in Mailbox");
        self.state.uid_next = mailbox.uid_next.expect("No UIDNEXT in Mailbox");
        self.state.save(&self.statefile)
    }

    pub fn get_last_seen_uid(&self) -> u32 {
        self.state.last_seen_uid
    }

    pub fn get_known_uids(&self) -> Result<HashSet<u32>, String> {
        let max_uid = self.get_last_seen_uid();
        let mut set = HashSet::with_capacity(max_uid as usize);
        let mut err = false;

        match std::fs::read_dir(self.dirpath.as_path()) {
            Err(e) => {
                eprintln!("Error: {}", e);
                err = true;
            }
            Ok(readdir) => {
                for direntry_res in readdir {
                    match direntry_res {
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            err = true;
                        }
                        Ok(direntry) => {
                            // Intentionally ignore conversion errors because
                            // there are some non-numeric files in the directory
                            if let Some(strname) = direntry.file_name().to_str() {
                                if let Ok(number) = u32::from_str_radix(strname, 10) {
                                    if number > 0 && number <= max_uid {
                                        set.insert(number);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if err {
            Err(format!("Could not read dir {}", self.dirpath.display()))
        } else {
            Ok(set)
        }
    }

    /*
    pub fn update_local_state(&self) {
        // TODO: Fill this in using the maildir stuff
        //       Find messages in the maildir that are not in the
        //       cache. Then push those messages to the mailbox.
    }
    */

    pub fn get_uid(&self, uid: Uid) -> Option<MessageMeta> {
        MessageMeta::from_file(&self.uidfile(uid)).ok()
    }

    pub fn delete_uid(&self, uid: u32) -> Result<(), String> {
        std::fs::remove_file(self.uidfile(uid)).map_err(|e| format!("{}", e))
    }

    // FIXME: Clean up the expect() in here to just return Err
    pub fn add_uid(&mut self, id: &str, fetch: &Fetch) -> Result<MessageMeta, String> {
        let uid = fetch.uid.expect("No UID in FETCH response");
        let size = fetch.size.expect("No SIZE in FETCH response");
        let flags = fetch.flags();
        let internal_date = fetch
            .internal_date()
            .expect("No INTERNALDATE in FETCH response");
        let path = self.uidfile(uid);
        if path.exists() {
            return Err(format!("UID {} already exists", uid));
        }
        let meta = MessageMeta::new(
            id,
            size,
            SyncFlags::from(flags),
            uid,
            internal_date.timestamp_millis(),
        );

        meta.save(&path).and_then(|_| {
            // We only remember the last seen uid after we have saved it
            if uid > self.state.last_seen_uid {
                self.state.last_seen_uid = uid;
                self.state.save(&self.statefile).map(|_| meta)
            } else {
                Ok(meta)
            }
        })
    }

    pub fn update_uid(&mut self, fetch: &Fetch) -> Result<MessageMeta, String> {
        let uid = fetch.uid.expect("No UID in FETCH response");
        self.get_uid(uid)
            .ok_or_else(|| format!("{}: Not Found", uid))
            .and_then(|mut meta| meta.update(&self.uidfile(uid), fetch).map(|_| meta))
    }

    pub fn delete_local_state(&self) -> Result<(), String> {
        // TODO: Clear all State and start over
        Ok(())
    }
}
