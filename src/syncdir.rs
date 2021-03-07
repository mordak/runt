use crate::cache::maildir_flags_from_imap;
use crate::cache::Cache;
use crate::cache::MessageMeta;
use crate::cache::SyncFlags;
use crate::config::Account;
use crate::imapw::{FetchResult, Imap, UidResult};
use crate::maildirw::Maildir;
use chrono::prelude::*;
use imap::types::{Fetch, Mailbox, Uid, UnsolicitedResponse, ZeroCopy};
use notify::{watcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::fs;
use std::ops::Deref;
use std::sync::mpsc::{channel, Receiver, RecvError, Sender, TryRecvError};
use std::thread::{sleep, spawn, JoinHandle};
use std::time::Duration;
use std::vec::Vec;

/// A enum used to pass messages between threads.
#[derive(Debug)]
pub enum SyncMessage {
    Exit,
    ImapChanged,
    ImapError(String),
    MaildirChanged,
    MaildirError(String),
}

/// A struct representing a single mailbox to synchronize
/// including the IMAP side and corresponding Maildir
pub struct SyncDir {
    pub config: Account,
    pub mailbox: String,
    pub sender: Sender<SyncMessage>,
    receiver: Receiver<SyncMessage>,
    cache: Cache,
    maildir: Maildir,
    idlethread: Option<JoinHandle<()>>,
    fsthread: Option<JoinHandle<()>>,
}

impl SyncDir {
    /// Make a new SyncDir from the given config and mailbox name
    pub fn new(config: &Account, mailbox: String) -> Result<SyncDir, String> {
        let myconfig = config.clone();
        let cache = Cache::new(&myconfig.account, &mailbox).unwrap();
        let maildir = Maildir::new(&myconfig.maildir, &myconfig.account, &mailbox)?;
        let (sender, receiver) = channel();
        Ok(SyncDir {
            config: myconfig,
            mailbox,
            sender,
            receiver,
            cache,
            maildir,
            idlethread: None,
            fsthread: None,
        })
    }

    /// Log a message to the console
    fn log(&self, msg: &str) {
        println!(
            "{} {}/{}: {}",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            self.config.account,
            self.mailbox,
            msg
        );
    }

    /// Log an error message to the console
    fn elog(&self, msg: &str) {
        eprintln!(
            "{} {}/{}: {}",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            self.config.account,
            self.mailbox,
            msg
        );
    }

    /// Spawn a thread on this mailbox and IDLE it. When the IDLE
    /// ends, the thread will send a message to the main sync thread.
    fn idle(&self) -> Result<JoinHandle<()>, String> {
        let mut imap = Imap::new(&self.config)?;
        imap.select_mailbox(&self.mailbox.as_str())?;
        //imap.debug(true);
        let sender = self.sender.clone();
        let handle = spawn(move || {
            if let Err(why) = imap.idle() {
                sender.send(SyncMessage::ImapError(why)).ok();
            }
            imap.logout().ok();
            sender.send(SyncMessage::ImapChanged).ok();
        });
        Ok(handle)
    }

    /// Check if we want to IDLE this mailbox
    fn should_idle(&self) -> bool {
        self.config.is_mailbox_idled(&self.mailbox)
    }

    /// Spawn a thread on this Maildir and wait for changes. On change,
    /// a message is sent to the parent the main sync thread.
    fn fswait(&self) -> Result<JoinHandle<()>, String> {
        let sender = self.sender.clone();
        let path = self.maildir.path();
        let handle = spawn(move || {
            let (tx, rx) = channel();
            let mut watcher = watcher(tx, Duration::from_secs(10)).unwrap();
            watcher.watch(path, RecursiveMode::Recursive).unwrap();
            loop {
                match rx.recv() {
                    Ok(event) => {
                        match event {
                            notify::DebouncedEvent::Write(path) if path.is_dir() => {
                                // trigger on dir writes only, which cover everything else
                                sender.send(SyncMessage::MaildirChanged).ok();
                            }
                            _ => (),
                        }
                    }
                    Err(e) => {
                        sender
                            .send(SyncMessage::MaildirError(format!("{:?}", e)))
                            .ok();
                    }
                }
            }
        });
        Ok(handle)
    }

    /// Save the given message in the Maildir.
    ///
    /// Updates the cache db on success. On failure, then we will
    /// refetch on the next loop.
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

    /// Delete a given UID from the Maildir and clear its entry from cache.
    ///
    /// Unconditionally deletes the cache db entry for this message after
    /// attempting to delete the message from the maildir. The most common
    /// cause of delete errors is the message already being deleted from the
    /// Maildir, so erroring prevents the cache db from being updated. In the
    /// event that deleting the message fails for some other reason, it will
    /// appear to be a new message in the Maildir and will be resynced on
    /// next sync. This might annoy the user, but errs on the side of caution
    /// when things go wrong.
    fn delete_message_from_maildir(&self, uid: u32) -> Result<(), String> {
        // It is ok if we can't find the message in our maildir, it
        // may be deleted from both sides.
        match self.cache.get_uid(uid) {
            Ok(meta) => {
                self.log(&format!("Deleting UID {} from maildir", uid));
                if let Err(why) = self.maildir.delete_message(meta.id()) {
                    self.elog(&format!("Error deleting UID {}: {}", uid, why));
                }
                self.cache.delete_uid(uid)
            }
            Err(e) => match e.downcast_ref::<rusqlite::Error>() {
                Some(rusqlite::Error::QueryReturnedNoRows) => Ok(()),
                _ => Err(e.to_string()),
            },
        }
    }

    /// Fetch the given UID from IMAP and save it in the Maildir.
    ///
    /// Used to fetch new messages from the server.
    fn cache_message_for_uid(&mut self, imap: &mut Imap, uid: Uid) -> Result<(), String> {
        imap.fetch_uid(uid).and_then(|zc_vec_fetch| {
            for fetch in zc_vec_fetch.deref() {
                self.log(&format!("Fetching UID {}: {:?}", uid, fetch.flags()));
                if let Err(e) = self.save_message_in_maildir(fetch) {
                    return Err(format!("Save UID {} in maildir failed: {}", uid, e));
                }
            }
            Ok(())
        })
    }

    /// Compare the given cache MessageMeta and IMAP UidResult, and decide if the
    /// cache version needs to be updated. If so, fetch the updated message and save
    /// it in the Maildir.
    ///
    /// Used to update cache entries for messages we already know about.
    fn update_cache_for_uid(
        &mut self,
        imap: &mut Imap,
        meta: &MessageMeta,
        uidres: &UidResult,
    ) -> Result<(), String> {
        // Check if anything has changed
        if meta.is_equal(uidres) {
            return Ok(());
        }

        if meta.needs_refetch(uidres) {
            // Pull down a whole new copy of the message.
            self.delete_message_from_maildir(meta.uid())?;
            self.cache_message_for_uid(imap, meta.uid())
        } else {
            self.log(&format!(
                "Updating UID {}: {:?} -> {:?}",
                uidres.uid(),
                meta.flags(),
                uidres.flags()
            ));
            self.cache.update(uidres).and_then(|newmeta| {
                if meta.needs_move_from_new_to_cur(uidres)
                    && self.maildir.message_is_in_new(meta.id())?
                {
                    self.maildir
                        .move_message_to_cur(meta.id(), &newmeta.flags())
                } else {
                    self.maildir
                        .set_flags_for_message(newmeta.id(), &newmeta.flags())
                }
            })
        }
    }

    /// For the given IMAP FETCH results, update the cache. Existing messages
    /// are updated if needed, and new messages are downloaded.
    ///
    /// Used to process a full set of IMAP FETCH results. Since the IMAP
    /// server is the source of truth, anything in the given FETCH results
    /// must be either existing / known or new and need to be downloaded.
    fn cache_uids_from_imap(
        &mut self,
        imap: &mut Imap,
        zc_vec_fetch: &ZeroCopy<Vec<Fetch>>,
    ) -> Result<(), String> {
        let mut err = false;
        for fetch in zc_vec_fetch.deref() {
            match FetchResult::from(fetch) {
                FetchResult::Uid(uidres) => {
                    let uid = uidres.uid();
                    let res = if let Ok(meta) = self.cache.get_uid(uid) {
                        self.update_cache_for_uid(imap, &meta, &uidres)
                    } else {
                        self.cache_message_for_uid(imap, uid)
                    };
                    if let Err(e) = res {
                        self.elog(&format!("Cache UID {} failed: {}", uid, e));
                        err = true;
                    }
                }
                FetchResult::Other(f) => self.log(&format!("Got Other FETCH response: {:?}", f)),
            }
        }
        if err {
            Err("Cache failed".to_string())
        } else {
            Ok(())
        }
    }

    /// Delete messages by UID from the cache and from the maildir.
    fn remove_uids_from_cache(&mut self, uids: &[u32]) -> Result<(), String> {
        for uid in uids {
            // Errors deleting from local usually mean the uid was not found
            // which can happen under some dual-edit conditions or when
            // we are told about a deleted message that we never downloded.
            if let Err(e) = self.delete_message_from_maildir(*uid) {
                self.elog(&format!("Error deleting UID {}: {}", uid, e));
            }
        }
        Ok(())
    }

    /// Check for VANISHED messages in the unsolicited responses channel
    fn check_unsolicited_for_vanished(
        &mut self,
        imap: &mut Imap,
    ) -> Result<Vec<std::ops::RangeInclusive<u32>>, String> {
        let mut vanished = Vec::new();
        imap.for_each_unsolicited_response(|u| {
            if let UnsolicitedResponse::Vanished {
                earlier: _,
                mut uids,
            } = u
            {
                vanished.append(&mut uids);
            }
        });
        Ok(vanished)
    }

    /// Compare the given IMAP FETCH results with the cache, and remove any entries
    /// from the cache that are no longer on the server.
    ///
    /// Called after processing the given fetch results and updating the
    /// cache db and Maildir. Any UIDs remaining in the cache db must have
    /// been deleted on the server and should be deleted from the cache db
    /// and the Maildir.
    fn remove_imap_deleted_messages(
        &mut self,
        zc_vec_fetch: &ZeroCopy<Vec<Fetch>>,
    ) -> Result<(), String> {
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
                            self.elog(&format!("UID {} exists on server but not in cache", uid));
                            err = true;
                        }
                    }
                    FetchResult::Other(f) => self.log(&format!("Got Other: {:?}", f)),
                }
            }

            // Remove uids from cache that have been removed on the server
            for uid in cached_uids {
                if let Err(e) = self.delete_message_from_maildir(uid) {
                    self.elog(&format!("Error deleting UID {}: {}", uid, e));
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

    /// Perform a sync from IMAP to the cache. This updates existing cache entries,
    /// removes messages deleted on the server, and downloads new messages.
    ///
    /// This is the main Server -> Local routine for UIDs. After this completes anything
    /// on the server will be in the cache db and in the Maildir.
    fn slow_sync_cache_from_imap(
        &mut self,
        imap: &mut Imap,
        mailbox: &Mailbox,
    ) -> Result<(), String> {
        let last_seen_uid = self.cache.get_last_seen_uid();
        let end: Option<u32> = match last_seen_uid {
            0 => None,
            x => Some(x),
        };

        // Updating existing cache entries
        imap.fetch_uids(1, end, None).and_then(|zc_vec_fetch| {
            if !self.cache.is_valid(mailbox) {
                // We have a new state, so delete the existing one
                self.delete_imap_cache()?;
            }
            self.cache_uids_from_imap(imap, &zc_vec_fetch)?;
            self.remove_imap_deleted_messages(&zc_vec_fetch)
        })?;

        // Fetch new messgaes
        imap.fetch_uids(last_seen_uid + 1, None, None)
            .and_then(|zc_vec_fetch| self.cache_uids_from_imap(imap, &zc_vec_fetch))?;

        self.cache.update_imap_state(mailbox)
    }

    /// Use QRESYNC to update the cache. This updates existing cache entries,
    /// removes deleted items on the server and downloads new messages.
    ///
    /// This is the main Server -> Local routine for UIDs. After this completes
    /// anything on the server will be in the cache db and in the Maildir.
    fn quick_sync_cache_from_imap(
        &mut self,
        imap: &mut Imap,
        mailbox: &Mailbox,
    ) -> Result<(), String> {
        let modseq = if self.cache.is_valid(mailbox) {
            Some(self.cache.get_highest_mod_seq())
        } else {
            self.delete_imap_cache()?;
            None
        };

        imap.fetch_uids(1, None, modseq)
            .and_then(|zc_vec_fetch| self.cache_uids_from_imap(imap, &zc_vec_fetch))?;

        self.check_unsolicited_for_vanished(imap).map(|vanished| {
            for range in vanished {
                for uid in range {
                    if let Err(e) = self.delete_message_from_maildir(uid) {
                        self.elog(&format!("Error deleting UID {}: {}", uid, e));
                    }
                }
            }
        })?;

        self.cache.update_imap_state(mailbox)
    }

    /// Delete the cache of the imap state.
    ///
    /// This is used when we have a cache validation failure, such as when
    /// the UIDVALIDITY does not match anymore.
    fn delete_imap_cache(&mut self) -> Result<(), String> {
        self.log("Deleting Cache of all IMAP messages");
        self.remove_uids_from_cache(
            &self
                .cache
                .get_known_uids()?
                .iter()
                .cloned()
                .collect::<Vec<u32>>(),
        )
    }

    /// Sync the Maildir with the cache. Locally deleted messages are deleted from
    /// the server, local changes are pushed to the server, and new messages are
    /// uploaded to the server.
    ///
    /// This is the main Local -> Server routine for Maildir IDs. Maildir entries
    /// are compared with the cache db and any changes in the Maildir are propagated
    /// to the server.
    fn sync_cache_from_maildir(&mut self, imap: &mut Imap) -> Result<(), String> {
        let mut ids = self.cache.get_known_ids()?;
        let (new, changed) = self.maildir.get_updates(&mut ids)?;
        let mut refetch = HashSet::<u32>::new();

        // ids now contains maildir entries that are in the cache
        // but not on the file system anymore. They need to be deleted
        // from the server.
        for meta in ids.values() {
            // delete from server
            self.log(&format!("Deleting UID {} from server", meta.uid()));
            imap.delete_uid(meta.uid())?;
            // delete from cache
            self.cache.delete_uid(meta.uid())?;
            // the change will come back to us on the IDLE
            // thread, but we'll just ignore it.
        }

        // changed contains maildir entries that are different on
        // disk than in the cache. These need to be synchronized
        // to the server.
        for id in changed {
            let cache_v = self.cache.get_id(&id)?;
            let mail_v = self.maildir.get_id(&id)?;

            // If we need to update flags then send changes.
            let cache_flags = SyncFlags::from(cache_v.flags().as_str());
            let maildir_flags = SyncFlags::from(mail_v.flags());
            let flags_diff = cache_flags.diff(maildir_flags);
            if let Some(flags) = flags_diff.add.as_imap_flags() {
                imap.add_flags_for_uid(cache_v.uid(), &flags)?;
                refetch.insert(cache_v.uid());
            }
            if let Some(flags) = flags_diff.sub.as_imap_flags() {
                imap.remove_flags_for_uid(cache_v.uid(), &flags)?;
                refetch.insert(cache_v.uid());
            }

            // If we need to push a new body.
            // FIXME: Can we use something better than size?
            //        If we store the file mod date, we could
            //        use that instead...
            if cache_v.size() as u64 != mail_v.size() {
                imap.replace_uid(
                    cache_v.uid(),
                    &fs::read(mail_v.path()).map_err(|e| e.to_string())?,
                )?;
                self.maildir.delete_message(&id)?;
                self.cache.delete_uid(cache_v.uid())?;
                refetch.remove(&cache_v.uid());
            }
        }

        // new contains maildir entries that are on the file system
        // but not in the cache. These need to be sent to the server.
        for id in new {
            let mail_v = self.maildir.get_id(&id)?;
            let sflags = SyncFlags::from(mail_v.flags());
            let flags = if let Some(f) = sflags.as_imap_flags() {
                f
            } else {
                Vec::new()
            };

            // Push to the server first, then delete the local copy
            imap.append(&fs::read(mail_v.path()).map_err(|e| e.to_string())?, &flags)?;
            // These will come back to us on the idle loop,
            // at which time they will get cache entries.
            self.maildir.delete_message(&id)?;
        }

        for uid in refetch {
            imap.fetch_uid_meta(uid)
                .and_then(|zc_vec_fetch| self.cache_uids_from_imap(imap, &zc_vec_fetch))?;
        }

        self.cache.update_maildir_state()
    }

    /// Run loop for the sync engine. Performs a full sync then waits on change
    /// events from the IMAP server or the Maildir.
    ///
    /// On each change, performs a sync between the server and the Mailfir.
    /// Each sync does a UID sync between the IMAP server and the cache db and
    /// Maildir. Then does a Maildir ID sync between the cache db and the IMAP
    /// server. The IMAP server knows about UIDs, and the Maildir knows about
    /// IDs. The cache db holds the mapping between these sets, and allows the
    /// sync engine to identify new and changed elements between each set.
    fn do_sync(&mut self) -> Result<(), String> {
        loop {
            let mut imap = Imap::new(&self.config)?;
            //imap.debug(true);
            if imap.can_qresync() {
                imap.enable_qresync().unwrap();
            }
            let mailbox = imap.select_mailbox(&self.mailbox.as_str())?;
            //imap.debug(false);

            self.log(&format!(
                "Synchronizing ({})",
                if imap.can_qresync() { "quick" } else { "slow" }
            ));
            let res = if imap.can_qresync() {
                self.quick_sync_cache_from_imap(&mut imap, &mailbox)
                    .and_then(|_| self.sync_cache_from_maildir(&mut imap))
                    .and_then(|_| imap.logout())
            } else {
                self.slow_sync_cache_from_imap(&mut imap, &mailbox)
                    .and_then(|_| self.sync_cache_from_maildir(&mut imap))
                    .and_then(|_| imap.logout())
            };

            self.log("Done");

            if let Err(e) = res {
                break Err(format!("Error syncing: {}", e));
            };

            // If we are not IDLEing, then we're done
            if !self.should_idle() {
                break Ok(());
            }

            if self.idlethread.is_none() {
                match self.idle() {
                    Ok(handle) => self.idlethread = Some(handle),
                    Err(why) => {
                        break Err(format!("Error in IDLE: {}", why));
                    }
                }
            }

            if self.fsthread.is_none() {
                match self.fswait() {
                    Ok(handle) => self.fsthread = Some(handle),
                    Err(why) => {
                        break Err(format!("Error in watching file system: {}", why));
                    }
                }
            }

            // Block until something happens
            let mut message = self.receiver.recv();

            // Then loop over all pending messages.
            // We do this because sometimes we get multiple notifications
            // from the file system that cause unnecessary synchronization
            loop {
                match message {
                    Ok(SyncMessage::Exit) => return Ok(()),
                    Ok(SyncMessage::ImapChanged) => {
                        self.log("IMAP changed");
                        if self.idlethread.is_some() {
                            self.idlethread.take().unwrap().join().ok();
                        }
                    }
                    Ok(SyncMessage::MaildirChanged) => {
                        self.log("Maildir changed");
                    }
                    Ok(SyncMessage::ImapError(msg)) => {
                        self.elog(&format!("IMAP Error: {}", msg));
                    }
                    Ok(SyncMessage::MaildirError(msg)) => {
                        self.elog(&format!("Maildir Error: {}", msg));
                    }
                    Err(why) => {
                        return Err(format!("Error in recv(): {}", why));
                    }
                }

                match self.receiver.try_recv() {
                    Err(TryRecvError::Empty) => break,
                    any => message = any.map_err(|_| RecvError),
                }
            }
        }
    }

    /// Public interface for the sync engine. Runs a sync loop until it exits.
    /// If the sync loop exited with an error, then it will respawn after a
    /// short delay.
    pub fn sync(&mut self) -> Result<(), String> {
        loop {
            match self.do_sync() {
                Err(why) => {
                    self.elog(&format!("Sync exited with error: {}", why));
                    // sleep 10 to throttle retries
                    sleep(Duration::from_secs(10));
                }
                Ok(_) => break Ok(()),
            }
        }
    }
}
