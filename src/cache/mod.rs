mod db;
mod messagemeta;
mod statefile;
mod syncflags;

pub use self::syncflags::SyncFlags;
use config::Config;
use imap::types::{Fetch, Flag, Mailbox};
use imapw::UidResult;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use self::db::Db;
pub use self::messagemeta::MessageMeta;
use self::statefile::StateFile;

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

/// Path to the db file for this cache
fn db_path(account: &str, mailbox: &str) -> PathBuf {
    let mut dbfile = self::path(account, mailbox);
    dbfile.push("db.sqlite");
    dbfile
}

/// Path to .state file for given account and mailbox
fn statefile(account: &str, mailbox: &str) -> PathBuf {
    let mut cachefile = self::path(account, mailbox);
    cachefile.push("state");
    cachefile
}

pub struct Cache {
    db: Db,
    state: StateFile,
}

impl Cache {
    pub fn new(account: &str, mailbox: &str) -> Result<Cache, String> {
        let db = Db::from_file(&self::db_path(account, mailbox))?;
        let state = StateFile::new(&self::statefile(account, mailbox))?;
        Ok(Cache { db, state })
    }

    pub fn is_valid(&self, mailbox: &Mailbox) -> bool {
        self.state.uid_validity() == mailbox.uid_validity.expect("No UIDVALIDITY in Mailbox")
    }

    pub fn update_imap_state(&mut self, mailbox: &Mailbox) -> Result<(), String> {
        self.state.update_imap(
            mailbox.uid_validity.expect("No UIDVALIDITY in Mailbox"),
            mailbox.uid_next.expect("No UIDNEXT in Mailbox"),
            mailbox
                .highest_mod_seq
                .expect("No HIGHESTMODSEQ in Mailbox"),
        )
    }

    pub fn get_last_seen_uid(&self) -> u32 {
        self.state.last_seen_uid()
    }

    /*
    pub fn get_highest_mod_seq(&self) -> u64 {
        self.state.highest_mod_seq()
    }

    pub fn set_highest_mod_seq(&mut self, seq: u64) -> Result<(), String> {
        if seq > self.state.highest_mod_seq {
            self.state.highest_mod_seq = seq;
            self.state.save(&self.statefile)
        } else {
            Ok(())
        }
    }
    */

    pub fn get_known_uids(&self) -> Result<HashSet<u32>, String> {
        self.db.get_uids()
    }

    pub fn get_known_ids(&self) -> Result<HashMap<String, MessageMeta>, String> {
        self.db.get_ids()
    }

    pub fn update_maildir_state(&mut self) -> Result<(), String> {
        self.state.update_maildir()
    }

    pub fn get_uid(&self, uid: u32) -> anyhow::Result<MessageMeta> {
        self.db.get_uid(uid)
    }

    pub fn delete_uid(&self, uid: u32) -> Result<(), String> {
        self.db.delete_uid(uid)
    }

    pub fn get_id(&self, id: &str) -> Result<MessageMeta, String> {
        self.db.get_id(id)
    }

    // FIXME: Clean up the expect() in here to just return Err
    pub fn add(&mut self, id: &str, fetch: &Fetch) -> Result<MessageMeta, String> {
        let uid = fetch.uid.expect("No UID in FETCH response");
        let size = fetch.size.expect("No SIZE in FETCH response");
        let flags = fetch.flags();
        let internal_date = fetch
            .internal_date()
            .expect("No INTERNALDATE in FETCH response");

        let meta = MessageMeta::new(
            id,
            size,
            SyncFlags::from(flags),
            uid,
            internal_date.timestamp_millis(),
        );

        self.db.add(&meta).and_then(|_| {
            // We only remember the last seen uid after we have saved it
            if uid > self.state.last_seen_uid() {
                self.state.set_last_seen_uid(uid).map(|_| meta)
            } else {
                Ok(meta)
            }
        })
    }

    pub fn update(&mut self, uidres: &UidResult) -> Result<MessageMeta, String> {
        let uid = uidres.uid();
        match self.get_uid(uid) {
            Ok(mut meta) => {
                if !meta.is_equal(uidres) {
                    meta.update(uidres);
                    self.db.update(&meta).map(|_| meta)
                } else {
                    Ok(meta)
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }
}
