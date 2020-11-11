use imap::types::{Flag, Uid};

use super::syncflags::{FlagValue, SyncFlags};
use crate::imapw::UidResult;

#[derive(Debug, Deserialize, Serialize)]
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

    pub fn update(&mut self, uidres: &UidResult) {
        self.uid = uidres.uid();
        self.size = uidres.size();
        self.internal_date_millis = uidres.internal_date_millis();
        self.flags = SyncFlags::from(uidres.flags());
    }

    pub fn flags_equal(&self, flags: &[Flag]) -> bool {
        let diff = self.flags.diff(SyncFlags::from(flags));
        diff.add.empty() && diff.sub.empty()
    }

    pub fn is_equal(&self, uidres: &UidResult) -> bool {
        self.uid == uidres.uid()
            && self.size == uidres.size()
            && self.internal_date_millis == uidres.internal_date_millis()
            && self.flags_equal(uidres.flags())
    }

    pub fn needs_refetch(&self, uidres: &UidResult) -> bool {
        self.size != uidres.size() || self.internal_date_millis != uidres.internal_date_millis()
    }

    pub fn needs_move_from_new_to_cur(&self, uidres: &UidResult) -> bool {
        !self.flags.contains(FlagValue::Seen) && uidres.flags().contains(&Flag::Seen)
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
