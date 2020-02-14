use imap::types::{Fetch, Flag, Uid};

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

    pub fn from_fields(
        uid: u32,
        size: u32,
        internal_date_millis: i64,
        flags: String,
        id: String,
    ) -> MessageMeta {
        MessageMeta {
            id,
            size,
            flags: SyncFlags::from(flags.as_str()),
            uid,
            internal_date_millis,
        }
    }

    pub fn update(&mut self, fetch: &Fetch) {
        self.size = fetch.size.expect("No SIZE in FETCH response");
        self.uid = fetch.uid.expect("No UID in FETCH response");
        self.internal_date_millis = fetch
            .internal_date()
            .expect("No internal_date in FETCH response")
            .timestamp_millis();
        self.flags = SyncFlags::from(fetch.flags());
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

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn internal_date_millis(&self) -> i64 {
        self.internal_date_millis
    }
}
