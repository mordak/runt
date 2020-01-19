use cache::maildir_flags_from_imap;
use cache::Cache;
use cache::MessageMeta;
use config::Config;
//use imap::error::Error as ImapError;
//use imap::error::Result as ImapResult;
use imap::types::{Fetch, Flag, Uid, ZeroCopy};
use imap::Session;
use maildir::Maildir;
use native_tls::TlsStream;
use std::net::TcpStream;
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Duration;
use std::vec::Vec;

pub struct SyncDir {
    pub config: Config,
    pub mailbox: String,
    session: Session<TlsStream<TcpStream>>,
    cache: Cache,
    maildir: Maildir,
}

impl SyncDir {
    // FIXME: Return Result
    pub fn new(config: &Config, mailbox: String) -> SyncDir {
        let myconfig = config.clone();
        let client = config.connect().unwrap();
        let session = client
            .login(
                myconfig.username.as_str(),
                myconfig.password.as_ref().unwrap(),
            )
            .unwrap();
        //session.debug = true;
        let cache = Cache::new(&myconfig.account, &mailbox).unwrap();
        let mut maildirpath = PathBuf::from(&myconfig.maildir);
        maildirpath.push(&mailbox);
        let maildir = Maildir::from(maildirpath);
        if let Err(e) = maildir.create_dirs() {
            panic!("Could not create maildir structure: {}", e);
        }
        SyncDir {
            config: myconfig,
            mailbox,
            session,
            cache,
            maildir,
        }
    }

    fn idle(&mut self) -> Result<(), String> {
        self.session
            .idle()
            .map_err(|e| format!("{}", e))
            .and_then(|mut i| {
                i.set_keepalive(Duration::from_secs(5 * 60));
                i.wait_keepalive().map_err(|e| format!("{}", e))
            })
    }

    fn save_message_in_maildir(&mut self, fetch: &Fetch) -> Result<MessageMeta, String> {
        fetch
            .body()
            .ok_or_else(|| format!("No BODY in FETCH result"))
            .and_then(|body| {
                if fetch.flags().contains(&Flag::Seen) {
                    self.maildir
                        .store_cur_with_flags(body, &maildir_flags_from_imap(fetch.flags()))
                } else {
                    self.maildir.store_new(body)
                }
                .map_err(|e| format!("Message store failed: {}", e))
                .and_then(|id| self.cache.add_uid(&id, &fetch))
            })
    }

    fn cache_message_for_uid(&mut self, uid: Uid) -> Result<(), String> {
        self.session
            .uid_fetch(
                format!("{}", uid),
                "(UID RFC822.SIZE INTERNALDATE FLAGS BODY.PEEK[])",
            )
            .map_err(|e| format!("{}", e))
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
            self.cache
                .update_uid(fetch)
                .and_then(|newmeta| {
                    self.maildir
                        .set_flags(newmeta.id(), &newmeta.flags())
                        .map_err(|e| format!("Set message flags failed: {}", e))
                })?;

            if meta.needs_move_from_new_to_cur(fetch) {
                println!("Moving {} {} from new to cur", meta.uid(), meta.id());
                self.maildir
                    .move_new_to_cur(meta.id())
                    .map_err(|e| format!("Move message id {} failed: {}", meta.id(), e))
            } else {
                Ok(())
            }
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
            .and_then(|meta| {
                self.maildir
                    .delete(meta.id())
                    .map_err(|e| format!("Maildir delete failed for UID {}: {}", uid, e))
            })?;

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
        let range = if last_seen_uid == 0 {
            "1:*".to_string()
        } else {
            format!("1:{}", last_seen_uid)
        };

        self.session
            .uid_fetch(range, "(UID FLAGS INTERNALDATE RFC822.SIZE)")
            .map_err(|e| format!("Refresh cache: {}", e))
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
            .uid_fetch(format!("{}:*", uid), "(UID FLAGS INTERNALDATE RFC822.SIZE)")
            .map_err(|e| format!("Get new messages: {}", e))
            .and_then(|zc_vec_fetch| self.cache_uids(&zc_vec_fetch))
    }

    fn push_local_changes(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub fn sync(&mut self) {
        self.session
            .select(&self.mailbox.as_str())
            .map_err(|e| format!("Could not SELECT {}: {}", self.mailbox, e))
            .and_then(|mailbox| {
                println!("Mailbox: {:?}", mailbox);

                let last_seen_uid = self.cache.get_last_seen_uid();

                // TODO: HIGHESTMODSEQ support

                self.refresh_cache(last_seen_uid, self.cache.is_valid(&mailbox))
                    .and_then(|_| self.cache.update_remote_state(&mailbox))
                    .and_then(|_| self.push_local_changes())
                    .and_then(|_| self.get_new_messages(last_seen_uid + 1))
                    //.and_then(|_| self.idle())
                    // FIXME: idle will return when the mailbox
                    // changes, so we will need to handle the changes
                    // and then loop again..
            })
            .unwrap_or_else(|e| eprintln!("Error syncing: {}", e));

        self.session.logout().unwrap();
    }
}
