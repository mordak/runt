use imap::types::{Fetch, Flag, Uid};
use std::io::Write;
use std::path::PathBuf;

use cache::syncflags::FlagValue;
use cache::syncflags::SyncFlags;

#[derive(Deserialize, Serialize)]
pub struct MessageMeta {
    id: String,
    size: u32,
    flags: SyncFlags,
    uid: Uid,
    internal_date_millis: i64,
}

impl MessageMeta {
    pub fn new(
        id: &str,
        size: u32,
        flags: SyncFlags,
        uid: Uid,
        internal_date_millis: i64,
    ) -> MessageMeta {
        MessageMeta {
            id: id.to_string(),
            size,
            flags,
            uid,
            internal_date_millis,
        }
    }

    pub fn from_file(path: &PathBuf) -> Result<MessageMeta, String> {
        std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {}", path.display(), e))
            .and_then(|buf| {
                serde_json::from_str(&buf).map_err(|e| format!("{}: {}", path.display(), e))
            })
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        std::fs::File::create(path)
            .and_then(|mut f| f.write_all(&serde_json::to_string_pretty(self).unwrap().as_bytes()))
            .map_err(|e| format!("{}: {}", path.display(), e))
    }

    pub fn flags_equal(&self, flags: &[Flag]) -> bool {
        let diff = self.flags.diff(SyncFlags::from(flags));
        diff.add.empty() && diff.sub.empty()
    }

    pub fn is_equal(&self, fetch: &Fetch) -> bool {
        self.size == fetch.size.expect("No SIZE in FETCH response")
            && self.uid == fetch.uid.expect("No UID in FETCH response")
            && self.internal_date_millis
                == fetch
                    .internal_date()
                    .expect("No INTERNALDATE in FETCH response")
                    .timestamp_millis()
            && self.flags_equal(fetch.flags())
    }

    pub fn needs_refetch(&self, fetch: &Fetch) -> bool {
        self.size != fetch.size.expect("No size in FETCH response")
            || self.internal_date_millis
                != fetch
                    .internal_date()
                    .expect("No INTERNALDATE in FETCH response")
                    .timestamp_millis()
    }

    pub fn update(&mut self, path: &PathBuf, fetch: &Fetch) -> Result<(), String> {
        self.flags = SyncFlags::from(fetch.flags());
        self.size = fetch.size.expect("No SIZE in FETCH response");
        self.internal_date_millis = fetch
            .internal_date()
            .expect("No INTERNALDATE in FETCH response")
            .timestamp_millis();
        self.save(path)
    }

    pub fn needs_move_from_new_to_cur(&self, fetch: &Fetch) -> bool {
        !self.flags.contains(FlagValue::Seen) && fetch.flags().contains(&Flag::Seen)
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn flags(&self) -> String {
        self.flags.to_string()
    }
}
