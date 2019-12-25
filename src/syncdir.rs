use cachefile::CacheFile;
use config::Config;
use std::time::Duration;

pub struct SyncDir {
    pub config: Config,
    pub mailbox: String,
}

impl SyncDir {
    pub fn new(config: &Config, mailbox: String) -> SyncDir {
        SyncDir {
            config: config.clone(),
            mailbox,
        }
    }

    pub fn sync(self) {
        let _cache = CacheFile::new(&self.config.account, &self.mailbox);

        let mut imap_client = self.config.connect().unwrap();

        imap_client.debug = true;
        let mut imap_session = imap_client
            .login(
                self.config.username.as_str(),
                self.config.password.unwrap().as_str(),
            )
            .unwrap();

        println!("Connected to mailbox: {}", self.mailbox);
        imap_session.select(&self.mailbox.as_str()).unwrap();
        //loop {
        {
            let unseen = imap_session.uid_search("UNSEEN 1:*").unwrap();
            println!("Got unseen: {:?}", unseen);
            let mut i = imap_session.idle().expect("Couldn't idle");
            i.set_keepalive(Duration::from_secs(10 * 60)); // FIXME: OpenBSD only accepts up to a little over 10 mins
            i.wait_keepalive().expect("Couldn't wait keepalive");
        }
        imap_session.logout().unwrap();
    }
}
