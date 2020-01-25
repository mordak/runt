use cache::maildir_flags_from_imap;
use cache::Cache;
use cache::MessageMeta;
use config::Config;
use maildirw::Maildir;
use imap::types::{Fetch, Uid, ZeroCopy};
use imapw::Session;
use std::ops::Deref;
use std::vec::Vec;

pub struct SyncDir {
    pub config: Config,
    pub mailbox: String,
    session: Session,
    cache: Cache,
    maildir: Maildir,
}

impl SyncDir {
    pub fn new(config: &Config, mailbox: String) -> Result<SyncDir, String> {
        let myconfig = config.clone();
        let session = Session::new(&myconfig)?;
        let cache = Cache::new(&myconfig.account, &mailbox).unwrap();
        let maildir = Maildir::new(&myconfig.maildir, &myconfig.account, &mailbox)?;
        Ok(SyncDir {
            config: myconfig,
            mailbox,
            session,
            cache,
            maildir,
        })
    }

    fn save_message_in_maildir(&mut self, fetch: &Fetch) -> Result<MessageMeta, String> {
        fetch
            .body()
            .ok_or_else(|| format!("No BODY in FETCH result"))
            .and_then(|body| {
                self.maildir
                    .save_message(body, &maildir_flags_from_imap(fetch.flags()))
            })
            .and_then(|id| self.cache.add_uid(&id, &fetch))
    }

    fn cache_message_for_uid(&mut self, uid: Uid) -> Result<(), String> {
        self.session
            .fetch_uid(uid)
            .and_then(|zc_vec_fetch| {
                for fetch in zc_vec_fetch.deref() {
                    eprintln!("Fetching UID {} FLAGS {:?}", uid, fetch.flags());
                    if let Err(e) = self.save_message_in_maildir(fetch) {
                        return Err(format!("Save UID {} failed: {}", uid, e));
                    }
                }
                Ok(())
            })
    }

    fn update_cache_for_uid(&mut self, meta: &MessageMeta, fetch: &Fetch) -> Result<(), String> {
        // Check if anything has changed
        if meta.is_equal(fetch) {
            return Ok(());
        }

        if meta.needs_refetch(fetch) {
            // Pull down a whole new copy of the message.
            self.delete_message(meta.uid())?;
            self.cache_message_for_uid(meta.uid())
        } else {
            println!("Updating UID {}", fetch.uid.expect("No UID"));
            self.cache.update_uid(fetch).and_then(|newmeta| {
                if meta.needs_move_from_new_to_cur(fetch) {
                    println!("Moving {} {} from new to cur", meta.uid(), meta.id());
                    self.maildir.move_message_to_cur(meta.id(), &newmeta.flags())
                } else {
                    self.maildir
                        .set_flags_for_message(newmeta.id(), &newmeta.flags())
                }
            })
        }
    }

    fn cache_uids(&mut self, zc_vec_fetch: &ZeroCopy<Vec<Fetch>>) -> Result<(), String> {
        let mut err = false;
        for fetch in zc_vec_fetch.deref() {
            let uid = fetch.uid.expect("No UID in FETCH response");
            let res = if let Some(meta) = self.cache.get_uid(uid) {
                self.update_cache_for_uid(&meta, fetch)
            } else {
                self.cache_message_for_uid(uid)
            };
            if let Err(e) = res {
                eprintln!("Cache UID {} failed: {}", uid, e);
                err = true;
            }
        }
        if err {
            Err("Cache failed".to_string())
        } else {
            Ok(())
        }
    }

    fn delete_message(&self, uid: u32) -> Result<(), String> {
        self.cache
            .get_uid(uid)
            .ok_or_else(|| format!("UID {} file disappeared?", uid))
            .and_then(|meta| self.maildir.delete_message(meta.id()))?;

        self.cache.delete_uid(uid)
    }

    fn remove_absent_uids(&mut self, zc_vec_fetch: &ZeroCopy<Vec<Fetch>>) -> Result<(), String> {
        let mut err = false;
        self.cache.get_known_uids().and_then(|mut cached_uids| {
            for fetch in zc_vec_fetch.deref() {
                let uid = fetch.uid.expect("No UID in FETCH");
                if !cached_uids.remove(&uid) {
                    eprintln!("UID {} exists on server but not in cache", uid);
                    err = true;
                }
            }
            for uid in cached_uids {
                println!("UID {} is gone on server", uid);
                if let Err(e) = self.delete_message(uid) {
                    eprintln!("Error deleting UID {}: {}", uid, e);
                    err = true;
                }
            }
            if err {
                Err("Error removing absent UIDs".to_string())
            } else {
                Ok(())
            }
        })
    }

    fn refresh_cache(&mut self, last_seen_uid: u32, uidvalid: bool) -> Result<(), String> {
        let end : Option<u32> = match last_seen_uid {
            0 => None,
            x => Some(x),
        };

        println!("Fetching UIDs {}:{:?}", 1, end);
        self.session
            .fetch_uids(1, end)
            .and_then(|zc_vec_fetch| {
                if !uidvalid {
                    // We have a new state, so delete the existing one
                    self.cache.delete_local_state()
                } else {
                    Ok(())
                }
                .and_then(|_| self.cache_uids(&zc_vec_fetch))
                .and_then(|_| self.remove_absent_uids(&zc_vec_fetch))
            })
    }

    fn get_new_messages(&mut self, uid: u32) -> Result<(), String> {
        self.session
            .fetch_uids(uid, None)
            .and_then(|zc_vec_fetch| self.cache_uids(&zc_vec_fetch))
    }

    fn push_local_changes(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub fn sync(&mut self) {
        self.session
            .select_mailbox(&self.mailbox.as_str())
            .and_then(|mailbox| {

                // TODO: HIGHESTMODSEQ support
                loop {
                    let last_seen_uid = self.cache.get_last_seen_uid();
                    let res = self.refresh_cache(last_seen_uid, self.cache.is_valid(&mailbox))
                        .and_then(|_| self.cache.update_remote_state(&mailbox))
                        .and_then(|_| self.push_local_changes())
                        .and_then(|_| self.get_new_messages(last_seen_uid + 1))
                        .and_then(|_| self.session.idle());

                    if res.is_err() {
                        eprintln!("Error syncing: {}", res.unwrap_err());
                        break;
                    };
                }
                Ok(())
            })
            .unwrap();

        self.session.logout().unwrap();
    }
}
