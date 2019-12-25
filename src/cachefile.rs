use std::io::Read;
use std::io::Write;
use std::path::PathBuf;

#[derive(Deserialize, Serialize)]
pub struct CacheFile {
    version: u64,
    remote_last: std::time::Duration,
    local_last: std::time::Duration,
}

impl CacheFile {
    pub fn new(account: &str, mailbox: &str) -> CacheFile {
        let path = CacheFile::path(account, mailbox);
        if path.exists() {
            CacheFile::from_file(&path)
        } else {
            CacheFile::make_new(&path)
        }
    }

    fn make_new(path: &PathBuf) -> CacheFile {
        let blank = CacheFile {
            version: 1,
            remote_last: std::time::Duration::new(0, 0),
            local_last: std::time::Duration::new(0, 0),
        };

        match std::fs::File::create(path) {
            Ok(mut f) => {
                f.write_all(&toml::to_vec(&blank).unwrap()).ok();
            }
            Err(e) => panic!("Could not create state file {:?}: {:?}", path, e),
        }
        blank
    }

    fn from_file(path: &PathBuf) -> CacheFile {
        let mut statefile = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) => panic!("Could not open cache file {:?} {:?}", path, e),
        };
        let mut buf: String = String::new();
        statefile.read_to_string(&mut buf).unwrap();
        toml::from_str(&buf).unwrap()
    }

    fn path(account: &str, mailbox: &str) -> PathBuf {
        let mut cachefile = match dirs::cache_dir() {
            Some(dir) => dir,
            _ => PathBuf::from(""),
        };
        cachefile.push("runt");
        cachefile.push(account);
        // Create the cache path if it doesn't exist
        std::fs::create_dir_all(&cachefile).ok();
        cachefile.push(mailbox);
        cachefile
    }
}
