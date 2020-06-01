use cache::MessageMeta;
use maildir::MailEntry;
use maildir::Maildir as SubMaildir;
use std::collections::HashMap;
use std::path::PathBuf;
//use std::time::SystemTime;

/// A wrapper around a maildir implementation
pub struct Maildir {
    maildir: SubMaildir,
}

/// A struct representing a mail message in the Maildir.
pub struct IdResult {
    //id: String,
    flags: String,
    size: u64,
    //modified_millis: u128,
    path: PathBuf,
}

impl IdResult {
    /*
    pub fn id(&self) -> &str {
        &self.id
    }
    */
    pub fn flags(&self) -> &str {
        &self.flags
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    /*
    pub fn modified_millis(&self) -> u128 {
        self.modified_millis
    }
    */
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// Determine if the given cache db entry for the message and the maildir
/// entry for the message are equivalent.
fn meta_equal(maildir_meta: &MailEntry, cache_meta: &MessageMeta) -> Result<bool, String> {
    if let Ok(fs_metadata) = maildir_meta.path().metadata() {
        if fs_metadata.len() != cache_meta.size() as u64 {
            return Ok(false);
        }
    } else {
        return Err(format!(
            "Could not get filesystem meta for {}",
            maildir_meta.id()
        ));
    }

    if maildir_meta.flags() != cache_meta.flags() {
        return Ok(false);
    }
    Ok(true)
}

impl Maildir {
    /// Make a new Maildir for the given root directory, account, and mailbox.
    pub fn new(root: &str, account: &str, mailbox: &str) -> Result<Maildir, String> {
        let mut maildirpath = PathBuf::from(root);
        maildirpath.push(account);
        maildirpath.push(mailbox);
        let maildir = SubMaildir::from(maildirpath);
        maildir
            .create_dirs()
            .map_err(|e| format!("Could not create maildir structure: {}", e))?;
        Ok(Maildir { maildir })
    }

    /// Get the path to the Maildir
    pub fn path(&self) -> PathBuf {
        self.maildir.path().to_path_buf()
    }

    /// Save a message in the maildir. On success, returns the ID of the new message.
    pub fn save_message(&mut self, body: &[u8], flags: &str) -> Result<String, String> {
        if flags.contains('S') {
            self.maildir.store_cur_with_flags(body, flags)
        } else {
            self.maildir.store_new(body)
        }
        .map_err(|e| format!("Message store failed: {}", e))
    }

    /// Move a message ID to the cur Maildir directory and set its flags.
    pub fn move_message_to_cur(&mut self, id: &str, flags: &str) -> Result<(), String> {
        self.maildir
            .move_new_to_cur_with_flags(id, flags)
            .map_err(|e| format!("Move message to cur failed for id{}: {}", id, e))
    }

    /// Set the flags for the given message ID.
    pub fn set_flags_for_message(&mut self, id: &str, flags: &str) -> Result<(), String> {
        self.maildir
            .set_flags(id, flags)
            .map_err(|e| format!("Setting flags failed for id {}: {}", id, e))
    }

    /// Delete a message ID.
    pub fn delete_message(&self, id: &str) -> Result<(), String> {
        self.maildir
            .delete(id)
            .map_err(|e| format!("Maildir delete failed for ID {}: {}", id, e))
    }

    /// For the given cached entries map (id -> meta), remove entries
    /// that have not changed, and return a vector of new ids not present
    /// in the cache.
    pub fn get_updates(
        &self,
        cache: &mut HashMap<String, MessageMeta>,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let mut new = Vec::new();
        let mut changed = Vec::new();
        for mailentry_res in self.maildir.list_new().chain(self.maildir.list_cur()) {
            let mailentry = mailentry_res.map_err(|e| e.to_string())?;

            if let Some(cache_meta) = cache.get(mailentry.id()) {
                // If the meta is different then add it to the changed list
                if !meta_equal(&mailentry, &cache_meta)? {
                    changed.push(mailentry.id().to_string());
                }

                // Remove the entry from the cachemap since it is still on disk.
                if cache.remove(mailentry.id()).is_none() {
                    return Err(format!("Cache id mismatch: {}", mailentry.id()));
                }
            } else {
                new.push(mailentry.id().to_string());
            }
        }
        Ok((new, changed))
    }

    /// Determine if a given message ID is in the Maildir 'new' folder.
    pub fn message_is_in_new(&self, id: &str) -> Result<bool, String> {
        for mailentry_res in self.maildir.list_new() {
            let mailentry = mailentry_res.map_err(|e| e.to_string())?;
            if mailentry.id() == id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Fetch the Maildir meta for the given message ID.
    pub fn get_id(&self, id: &str) -> Result<IdResult, String> {
        if let Some(entry) = self.maildir.find(id) {
            let meta = entry.path().metadata().map_err(|e| e.to_string())?;

            let size = meta.len();
            /*
            let modified_millis = meta
                .modified()
                .map_err(|e| e.to_string())?
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis();
            */

            Ok(IdResult {
                //id: entry.id().to_string(),
                flags: entry.flags().to_string(),
                size,
                //modified_millis,
                path: entry.path().clone(),
            })
        } else {
            Err(format!("Not found: {}", id))
        }
    }
}
