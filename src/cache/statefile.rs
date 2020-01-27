use std::io::Write;
use std::path::PathBuf;

#[derive(Deserialize, Serialize)]
pub struct StateFile {
    pub version: u64,
    pub remote_last: i64,
    pub local_last: i64,
    pub uid_validity: u32,
    pub uid_next: u32,
    pub last_seen_uid: u32,
    pub highest_mod_seq: u64,
}

impl StateFile {
    pub fn new(path: &PathBuf) -> Result<StateFile, String> {
        if path.exists() {
            StateFile::from_file(&path)
        } else {
            StateFile::make_new(&path)
        }
    }

    fn make_new(path: &PathBuf) -> Result<StateFile, String> {
        let blank = StateFile {
            version: 1,
            remote_last: 0,
            local_last: 0,
            uid_validity: 0,
            uid_next: 0,
            last_seen_uid: 0,
            highest_mod_seq: 0,
        };
        blank.save(path).map(|_| blank)
    }

    fn from_file(path: &PathBuf) -> Result<StateFile, String> {
        std::fs::read_to_string(path)
            .map_err(|e| format!("{}", e))
            .and_then(|buf| serde_json::from_str(&buf).map_err(|e| format!("{}", e)))
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        std::fs::File::create(path)
            .and_then(|mut f| f.write_all(&serde_json::to_string_pretty(self).unwrap().as_bytes()))
            .map_err(|e| format!("{}", e))
    }
}
