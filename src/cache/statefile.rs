use std::io::Write;
use std::path::{Path, PathBuf};

pub struct StateFile {
    path: PathBuf,
    state: StateFileFields,
}

#[derive(Deserialize, Serialize)]
pub struct StateFileFields {
    version: u64,
    imap_last: i64,
    maildir_last: i64,
    uid_validity: u32,
    uid_next: u32,
    last_seen_uid: u32,
    highest_mod_seq: u64,
}

impl StateFile {
    pub fn new(path: &Path) -> Result<StateFile, String> {
        if path.exists() {
            StateFile::from_file(&path)
        } else {
            StateFile::make_new(&path)
        }
    }

    fn make_new(path: &Path) -> Result<StateFile, String> {
        let blank = StateFile {
            path: path.to_path_buf(),
            state: StateFileFields {
                version: 1,
                imap_last: 0,
                maildir_last: 0,
                uid_validity: 0,
                uid_next: 0,
                last_seen_uid: 0,
                highest_mod_seq: 0,
            },
        };
        blank.save().map(|_| blank)
    }

    fn from_file(path: &Path) -> Result<StateFile, String> {
        std::fs::read_to_string(path)
            .map_err(|e| format!("{}", e))
            .and_then(|buf| serde_json::from_str(&buf).map_err(|e| format!("{}", e)))
            .map(|state| StateFile {
                path: path.to_path_buf(),
                state,
            })
    }

    pub fn update_imap(
        &mut self,
        uid_validity: u32,
        uid_next: u32,
        highest_mod_seq: u64,
    ) -> Result<(), String> {
        self.state.imap_last = chrono::offset::Utc::now().timestamp_millis();
        self.state.uid_validity = uid_validity;
        self.state.uid_next = uid_next;
        self.state.highest_mod_seq = highest_mod_seq;
        self.save()
    }

    pub fn update_maildir(&mut self) -> Result<(), String> {
        self.state.maildir_last = chrono::offset::Utc::now().timestamp_millis();
        self.save()
    }

    pub fn set_last_seen_uid(&mut self, uid: u32) -> Result<(), String> {
        self.state.last_seen_uid = uid;
        self.save()
    }

    /*
    pub fn set_highest_mod_seq(&mut self, seq: u64) -> Result<(), String> {
        self.state.highest_mod_seq = seq;
        self.save()
    }
    */

    pub fn save(&self) -> Result<(), String> {
        std::fs::File::create(&self.path)
            .and_then(|mut f| {
                f.write_all(
                    &serde_json::to_string_pretty(&self.state)
                        .unwrap()
                        .as_bytes(),
                )
            })
            .map_err(|e| format!("{}", e))
    }

    /*
    pub fn imap_last(&self) -> i64 {
        self.state.imap_last
    }
    pub fn maildir_last(&self) -> i64 {
        self.state.maildir_last
    }
    */
    pub fn uid_validity(&self) -> u32 {
        self.state.uid_validity
    }
    /*
    pub fn uid_next(&self) -> u32 {
        self.state.uid_next
    }
    */
    pub fn last_seen_uid(&self) -> u32 {
        self.state.last_seen_uid
    }

    pub fn highest_mod_seq(&self) -> u64 {
        self.state.highest_mod_seq
    }
}
