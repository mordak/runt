use cache::maildir_flags_from_imap;
use cache::Cache;
use cache::MessageMeta;
use config::Config;
use imap::types::{Fetch, Mailbox, Uid, ZeroCopy};
use imapw::{FetchResult, Session, UidResult};
use maildirw::Maildir;
use std::ops::Deref;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{spawn, JoinHandle};
use std::vec::Vec;

#[derive(Debug)]
pub enum SyncMessage {
    Exit,
    ImapChanged,
    MaildirChanged,
}

pub struct SyncDir {
    pub config: Config,
    pub mailbox: String,
    pub sender: Sender<SyncMessage>,
    receiver: Receiver<SyncMessage>,
    session: Session,
    cache: Cache,
    maildir: Maildir,
    idlethread: Option<JoinHandle<()>>,
}

impl SyncDir {
    pub fn new(config: &Config, mailbox: String) -> Result<SyncDir, String> {
        let myconfig = config.clone();
        let session = Session::new(&myconfig)?;
        let cache = Cache::new(&myconfig.account, &mailbox).unwrap();
        let maildir = Maildir::new(&myconfig.maildir, &myconfig.account, &mailbox)?;
        let (sender, receiver) = channel();
        Ok(SyncDir {
            config: myconfig,
            mailbox,
            sender,
            receiver,
            session,
            cache,
            maildir,
            idlethread: None,
        })
    }

    fn idle(&self) -> Result<JoinHandle<()>, String> {
        let mut session = Session::new(&self.config)?;
        session.select_mailbox(&self.mailbox.as_str())?;
        let sender = self.sender.clone();
        let handle = spawn(move || {
            println!("IDLE");
            if let Err(why) = session.idle() {
                eprintln!("Error in session IDLE: {}", why);
            }
            println!("IDLE Done");
            session.logout().ok();
            sender.send(SyncMessage::ImapChanged).ok();
        });
        Ok(handle)
    }

    fn save_message_in_maildir(&mut self, fetch: &Fetch) -> Result<MessageMeta, String> {
        fetch
            .body()
            .ok_or_else(|| "No BODY in FETCH result".to_string())
            .and_then(|body| {
                self.maildir
                    .save_message(body, &maildir_flags_from_imap(fetch.flags()))
            })
            .and_then(|id| self.cache.add(&id, &fetch))
    }

    fn cache_message_for_uid(&mut self, uid: Uid) -> Result<(), String> {
        self.session.fetch_uid(uid).and_then(|zc_vec_fetch| {
            for fetch in zc_vec_fetch.deref() {
                eprintln!("Fetching UID {} FLAGS {:?}", uid, fetch.flags());
                if let Err(e) = self.save_message_in_maildir(fetch) {
                    return Err(format!("Save UID {} failed: {}", uid, e));
                }
            }
            Ok(())
        })
    }

    fn update_cache_for_uid(
        &mut self,
        meta: &MessageMeta,
        uidres: &UidResult,
    ) -> Result<(), String> {
        // Check if anything has changed
        if meta.is_equal(uidres) {
            return Ok(());
        }

        if meta.needs_refetch(uidres) {
            // Pull down a whole new copy of the message.
            self.delete_message(meta.uid())?;
            self.cache_message_for_uid(meta.uid())
        } else {
            println!("Updating UID {}", uidres.uid());
            self.cache.update(uidres).and_then(|newmeta| {
                if meta.needs_move_from_new_to_cur(uidres)
                    && self.maildir.message_is_in_new(meta.id())?
                {
                    println!("Moving {} {} from new to cur", meta.uid(), meta.id());
                    dbg!(self
                        .maildir
                        .move_message_to_cur(meta.id(), &newmeta.flags()))
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
            match FetchResult::from(fetch) {
                FetchResult::Uid(uidres) => {
                    let uid = uidres.uid();
                    let res = if let Ok(meta) = self.cache.get_uid(uid) {
                        self.update_cache_for_uid(&meta, &uidres)
                    } else {
                        self.cache_message_for_uid(uid)
                    };
                    if let Err(e) = res {
                        eprintln!("Cache UID {} failed: {}", uid, e);
                        err = true;
                    }
                }
                FetchResult::Other(f) => println!("Got Other: {:?}", f),
            }
        }
        if err {
            Err("Cache failed".to_string())
        } else {
            Ok(())
        }
    }

    fn delete_message(&self, uid: u32) -> Result<(), String> {
        println!("Deleting: {}", uid);
        dbg!(self
            .cache
            .get_uid(uid)
            .and_then(|meta| self.maildir.delete_message(meta.id()))
            .and_then(|_| self.cache.delete_uid(uid)))
    }

    fn remove_absent_uids(&mut self, zc_vec_fetch: &ZeroCopy<Vec<Fetch>>) -> Result<(), String> {
        let mut err = false;
        self.cache.get_known_uids().and_then(|mut cached_uids| {
            // Remove all the fetched uids from the cached values
            // leaving only uids that are in the cache but not on
            // the server anymore.
            for fetch in zc_vec_fetch.deref() {
                match FetchResult::from(fetch) {
                    FetchResult::Uid(uidres) => {
                        let uid = uidres.uid();
                        if !cached_uids.remove(&uid) {
                            eprintln!("UID {} exists on server but not in cache", uid);
                            err = true;
                        }
                    }
                    FetchResult::Other(f) => println!("Got Other: {:?}", f),
                }
            }

            // Remove uids from cache that have been removed on the server
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

    fn refresh_cache(&mut self, last_seen_uid: u32, mailbox: &Mailbox) -> Result<(), String> {
        let end: Option<u32> = match last_seen_uid {
            0 => None,
            x => Some(x),
        };

        println!("Fetching UIDs 1:{:?}", end);
        self.session.fetch_uids(1, end).and_then(|zc_vec_fetch| {
            if !self.cache.is_valid(mailbox) {
                // We have a new state, so delete the existing one
                self.cache.delete_maildir_state()
            } else {
                Ok(())
            }
            .and_then(|_| self.cache_uids(&zc_vec_fetch))
            .and_then(|_| self.remove_absent_uids(&zc_vec_fetch))
            .and_then(|_| self.cache.update_imap_state(mailbox))
        })
    }

    fn get_new_messages(&mut self, uid: u32) -> Result<(), String> {
        self.session
            .fetch_uids(uid, None)
            .and_then(|zc_vec_fetch| self.cache_uids(&zc_vec_fetch))
    }

    fn push_maildir_changes(&mut self) -> Result<(), String> {
        let mut ids = self.cache.get_known_ids()?;
        let _new = self.maildir.get_updates(&mut ids)?;

        // TODO: For each remaining ids, update the server with the
        //       new values. For each new, store the message on the
        //       server and create an entry for the new message.
        //       Is there a sane way to round-trip the messages
        //       through the server to get them normal names, etc?

        self.cache.update_maildir_state()
    }

    pub fn sync(&mut self) {
        self.session.debug(true);
        self.session.enable_qresync().unwrap();
        self.session.debug(true);
        self.session
            .select_mailbox(&self.mailbox.as_str())
            .and_then(|mailbox| {
                // TODO: HIGHESTMODSEQ support
                loop {
                    let last_seen_uid = self.cache.get_last_seen_uid();
                    dbg!(last_seen_uid);
                    let res = self
                        .refresh_cache(last_seen_uid, &mailbox)
                        .and_then(|_| self.push_maildir_changes())
                        .and_then(|_| self.get_new_messages(last_seen_uid + 1));

                    if let Err(e) = res {
                        eprintln!("Error syncing: {}", e);
                        break;
                    };

                    match self.idle() {
                        Ok(handle) => self.idlethread = Some(handle),
                        Err(why) => {
                            eprintln!("Error in IDLE: {}", why);
                            break;
                        }
                    }

                    match self.receiver.recv() {
                        Ok(SyncMessage::Exit) => break,
                        Ok(SyncMessage::ImapChanged) => {
                            println!("IDLE thread exited");
                            if self.idlethread.is_some() {
                                self.idlethread.take().unwrap().join().ok();
                            }
                        }
                        Ok(m) => {
                            eprintln!("Unexpected message in SyncDir: {:?}", m);
                            break;
                        }
                        Err(why) => {
                            eprintln!("Error in recv(): {}", why);
                            break;
                        }
                    }
                }
                Ok(())
            })
            .unwrap();

        self.session.logout().unwrap();
    }
}
