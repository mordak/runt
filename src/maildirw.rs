use cache::MessageMeta;
use maildir::MailEntry;
use maildir::Maildir as SubMaildir;
use std::collections::HashMap;
use std::path::PathBuf;

/// A wrapper around a maildir implementation
pub struct Maildir {
    maildir: SubMaildir,
}

/*
pub struct MaildirEntry {
    id: String,
    flags: String,
    size: String,
    modified: i64,
}
*/

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

    pub fn save_message(&mut self, body: &[u8], flags: &str) -> Result<String, String> {
        if flags.contains('S') {
            self.maildir.store_cur_with_flags(body, flags)
        } else {
            self.maildir.store_new(body)
        }
        .map_err(|e| format!("Message store failed: {}", e))
    }

    pub fn move_message_to_cur(&mut self, id: &str, flags: &str) -> Result<(), String> {
        self.maildir
            .move_new_to_cur_with_flags(id, flags)
            .map_err(|e| format!("Move message to cur failed for id{}: {}", id, e))
    }

    pub fn set_flags_for_message(&mut self, id: &str, flags: &str) -> Result<(), String> {
        self.maildir
            .set_flags(id, flags)
            .map_err(|e| format!("Setting flags failed for id {}: {}", id, e))
    }

    pub fn delete_message(&self, id: &str) -> Result<(), String> {
        self.maildir
            .delete(id)
            .map_err(|e| format!("Maildir delete failed for ID {}: {}", id, e))
    }

    // For the given cached entries map (id -> meta), remove entries
    // that have not changed, and return a vector of new ids not present
    // in the cache.
    pub fn get_updates(
        &self,
        cache: &mut HashMap<String, MessageMeta>,
    ) -> Result<Vec<String>, String> {
        let mut v = Vec::new();
        for mailentry_res in self.maildir.list_new().chain(self.maildir.list_cur()) {
            let mailentry = mailentry_res.map_err(|e| e.to_string())?;

            if let Some(cache_meta) = cache.get(mailentry.id()) {
                if meta_equal(&mailentry, &cache_meta)? {
                    if cache.remove(mailentry.id()).is_none() {
                        return Err(format!("Cache id mismatch: {}", mailentry.id()));
                    }
                }
            } else {
                v.push(mailentry.id().to_string());
            }
        }
        Ok(v)
    }
}
