use imap::types::Flag;
use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::ser::{Serialize, Serializer};

#[derive(Serialize, Deserialize, PartialEq, Clone, Copy, Debug)]
pub enum FlagValue {
    NoFlag = 0,
    Draft = 0x44,
    Flagged = 0x46,
    Replied = 0x52,
    Seen = 0x53,
    Trashed = 0x54,
}

#[derive(Debug)]
pub struct SyncFlags {
    maildir: [FlagValue; 5],
}

impl Serialize for SyncFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct SyncFlagsVisitor;

impl<'de> Visitor<'de> for SyncFlagsVisitor {
    type Value = SyncFlags;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(r#"maildir: "DFRST" where all letters are optional"#)
    }

    fn visit_str<E>(self, value: &str) -> Result<SyncFlags, E>
    where
        E: de::Error,
    {
        Ok(SyncFlags::from(value))
    }
}

impl<'de> Deserialize<'de> for SyncFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(SyncFlagsVisitor)
    }
}

impl SyncFlags {
    fn new() -> SyncFlags {
        SyncFlags {
            maildir: [FlagValue::NoFlag; 5],
        }
    }
}

impl From<&str> for SyncFlags {
    fn from(s: &str) -> SyncFlags {
        let mut flags = SyncFlags::new();
        for b in s.bytes() {
            match b {
                b'D' => flags.maildir[0] = FlagValue::Draft,
                b'F' => flags.maildir[1] = FlagValue::Flagged,
                b'R' => flags.maildir[2] = FlagValue::Replied,
                b'S' => flags.maildir[3] = FlagValue::Seen,
                b'T' => flags.maildir[4] = FlagValue::Trashed,
                _ => (),
            }
        }
        flags
    }
}

impl From<&[Flag<'_>]> for SyncFlags {
    fn from(imap_flags: &[Flag]) -> SyncFlags {
        let mut flags = SyncFlags::new();
        for f in imap_flags {
            match f {
                Flag::Seen => flags.maildir[3] = FlagValue::Seen,
                Flag::Answered => flags.maildir[2] = FlagValue::Replied,
                Flag::Flagged => flags.maildir[1] = FlagValue::Flagged,
                Flag::Deleted => flags.maildir[4] = FlagValue::Trashed,
                Flag::Draft => flags.maildir[0] = FlagValue::Draft,
                _ => (),
            }
        }
        flags
    }
}

impl ToString for SyncFlags {
    fn to_string(&self) -> String {
        let mut s = String::with_capacity(5);
        for i in 0..self.maildir.len() {
            match self.maildir[i] {
                FlagValue::Draft => s.push('D'),
                FlagValue::Flagged => s.push('F'),
                FlagValue::Replied => s.push('R'),
                FlagValue::Seen => s.push('S'),
                FlagValue::Trashed => s.push('T'),
                _ => (),
            }
        }
        s
    }
}

impl SyncFlags {
    pub fn contains(&self, other: FlagValue) -> bool {
        for flag in &self.maildir {
            if *flag == other {
                return true;
            }
        }
        false
    }

    pub fn diff(&self, other: SyncFlags) -> SyncFlagsDiff {
        let mut diff = SyncFlagsDiff::new();
        for i in 0..self.maildir.len() {
            match (self.maildir[i], other.maildir[i]) {
                (FlagValue::NoFlag, FlagValue::NoFlag) => (),
                (FlagValue::NoFlag, x) => diff.add.maildir[i] = x,
                (x, FlagValue::NoFlag) => diff.sub.maildir[i] = x,
                _ => (),
            }
        }
        diff
    }

    pub fn empty(&self) -> bool {
        for flag in &self.maildir {
            if *flag != FlagValue::NoFlag {
                return false;
            }
        }
        true
    }

    pub fn as_imap_flags(&self) -> Option<Vec<Flag>> {
        let mut res = Vec::<Flag>::with_capacity(self.maildir.len());
        for flag in &self.maildir {
            match *flag {
                FlagValue::NoFlag => (),
                FlagValue::Draft => res.push(Flag::Draft),
                FlagValue::Flagged => res.push(Flag::Flagged),
                FlagValue::Replied => res.push(Flag::Answered),
                FlagValue::Seen => res.push(Flag::Seen),
                FlagValue::Trashed => res.push(Flag::Deleted),
            }
        }
        if !res.is_empty() {
            Some(res)
        } else {
            None
        }
    }
}

pub struct SyncFlagsDiff {
    pub add: SyncFlags,
    pub sub: SyncFlags,
}

impl SyncFlagsDiff {
    fn new() -> SyncFlagsDiff {
        SyncFlagsDiff {
            add: SyncFlags::new(),
            sub: SyncFlags::new(),
        }
    }
}
