use maildir::Maildir as SubMaildir;
use std::path::PathBuf;

/// A wrapper around a maildir implementation
pub struct Maildir {
    maildir: SubMaildir,
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

    pub fn move_message_to_cur(&mut self, id: &str) -> Result<(), String> {
        self.maildir
            .move_new_to_cur(id)
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
}
