use config::Config;
use imap::types::{Fetch, Flag, Mailbox, Name, Uid, ZeroCopy};
use imap::Client;
use imap::Session;
use imap_proto::types::Capability;
use native_tls::TlsConnector;
use native_tls::TlsStream;
use std::convert::From;
use std::net::TcpStream;
use std::ops::Deref;
use std::time::Duration;
use std::vec::Vec;

pub enum FetchResult<'a> {
    Uid(UidResult<'a>),
    //    ModSeq(ModResult),
    Other(&'a Fetch),
}

#[derive(Debug)]
pub struct UidResult<'a> {
    fetch: &'a Fetch,
}

impl<'a> UidResult<'a> {
    pub fn uid(&self) -> Uid {
        self.fetch.uid.unwrap()
    }
    pub fn size(&self) -> u32 {
        self.fetch.size.unwrap()
    }
    pub fn internal_date_millis(&self) -> i64 {
        self.fetch.internal_date().unwrap().timestamp_millis()
    }
    pub fn flags(&self) -> &[Flag] {
        self.fetch.flags()
    }
}

impl<'a> From<&'a Fetch> for FetchResult<'a> {
    fn from(fetch: &'a Fetch) -> FetchResult<'a> {
        // FIXME: Handle MODSEQ here
        if fetch.uid.is_some() && fetch.size.is_some() && fetch.internal_date().is_some() {
            FetchResult::Uid(UidResult { fetch })
        } else {
            FetchResult::Other(fetch)
        }
    }
}

fn flag2string(flag: &Flag) -> Option<String> {
    match flag {
        Flag::Seen => Some("\\Seen".to_string()),
        Flag::Answered => Some("\\Answered".to_string()),
        Flag::Flagged => Some("\\Flagged".to_string()),
        Flag::Deleted => Some("\\Deleted".to_string()),
        Flag::Draft => Some("\\Draft".to_string()),
        Flag::Recent => Some("\\Recent".to_string()),
        Flag::Custom(s) => Some(s.to_string()),
        _ => None,
    }
}

pub struct Imap {
    session: Session<TlsStream<TcpStream>>,
    mailbox: Option<String>,
}

impl Imap {
    pub fn new(config: &Config) -> Result<Imap, String> {
        let client = Imap::connect(config)?;
        let mut session = client
            .login(config.username.as_str(), config.password.as_ref().unwrap())
            .map_err(|e| format!("Login failed: {:?}", e))?;

        let capabilities = session
            .capabilities()
            .map_err(|e| format!("CAPABILITIES Error: {}", e))?;
        if !capabilities.deref().has(&Capability::Atom("QRESYNC"))
            || !capabilities.deref().has(&Capability::Atom("ENABLE"))
            || !capabilities.deref().has(&Capability::Atom("UIDPLUS"))
            || !capabilities.deref().has(&Capability::Atom("IDLE"))
        {
            return Err("Missing CAPABILITY support".to_string());
        }

        Ok(Imap {
            session,
            mailbox: None,
        })
    }

    #[allow(dead_code)]
    pub fn debug(&mut self, enable: bool) {
        self.session.debug = enable;
    }

    fn connect(config: &Config) -> Result<Client<TlsStream<TcpStream>>, String> {
        let socket_addr = (config.server.as_str(), config.port.unwrap());

        let mut tlsconnector = TlsConnector::builder();
        if config.server_ca_path.is_some() {
            tlsconnector.add_root_certificate(config.get_server_ca_cert().unwrap());
        }
        let tls = tlsconnector.build().unwrap();

        imap::connect(socket_addr, config.server.as_str(), &tls)
            .map_err(|e| format!("Connection to {:?} failed: {}", socket_addr, e))
    }

    pub fn list(
        &mut self,
        reference_name: Option<&str>,
        mailbox_pattern: Option<&str>,
    ) -> Result<ZeroCopy<Vec<Name>>, String> {
        self.session
            .list(reference_name, mailbox_pattern)
            .map_err(|e| format!("LIST failed: {}", e))
    }

    pub fn idle(&mut self) -> Result<(), String> {
        self.session
            .idle()
            .map_err(|e| format!("{}", e))
            .and_then(|mut i| {
                i.set_keepalive(Duration::from_secs(10 * 60));
                i.wait_keepalive().map_err(|e| format!("{}", e))
            })
    }

    pub fn fetch_uid(&mut self, uid: u32) -> Result<ZeroCopy<Vec<Fetch>>, String> {
        self.session
            .uid_fetch(
                format!("{}", uid),
                "(UID RFC822.SIZE INTERNALDATE FLAGS BODY.PEEK[])",
            )
            .map_err(|e| format!("UID FETCH failed: {}", e))
    }

    pub fn fetch_uid_meta(&mut self, uid: u32) -> Result<ZeroCopy<Vec<Fetch>>, String> {
        self.session
            .uid_fetch(format!("{}", uid), "(UID RFC822.SIZE INTERNALDATE FLAGS)")
            .map_err(|e| format!("UID FETCH failed: {}", e))
    }

    pub fn fetch_uids(
        &mut self,
        first: u32,
        last: Option<u32>,
    ) -> Result<ZeroCopy<Vec<Fetch>>, String> {
        let range = match last {
            None => format!("{}:*", first),
            Some(n) if n > first => format!("{}:{}", first, n),
            _ => return Err(format!("Invalid range {}:{}", first, last.unwrap())),
        };

        self.session
            .uid_fetch(range, "(UID RFC822.SIZE INTERNALDATE FLAGS)")
            .map_err(|e| format!("UID FETCH failed: {}", e))
    }

    pub fn enable_qresync(&mut self) -> Result<(), String> {
        self.session
            .run_command_and_check_ok("ENABLE QRESYNC")
            .map_err(|e| format!("ENABLE QRESYNC Error: {}", e))
    }

    pub fn select_mailbox(&mut self, mailbox: &str) -> Result<Mailbox, String> {
        self.session
            .select(mailbox)
            .map_err(|e| format!("SELECT {} failed: {}", mailbox, e))
            .and_then(|mbox| {
                self.mailbox = Some(mailbox.to_string());
                Ok(mbox)
            })
    }

    pub fn logout(&mut self) -> Result<(), String> {
        self.session
            .logout()
            .map_err(|e| format!("LOGOUT failed: {}", e))
    }

    pub fn delete_uid(&mut self, uid: u32) -> Result<(), String> {
        self.session
            .uid_store(format!("{}", uid), "+FLAGS (\\Deleted)")
            .map_err(|e| format!("STORE UID {} +Deleted failed: {}", uid, e))?;
        self.session
            .uid_expunge(format!("{}", uid))
            .map_err(|e| format!("EXPUNGE UID {} failed: {}", uid, e))?;
        Ok(())
    }

    pub fn append(&mut self, body: &[u8], flags: Option<&[String]>) -> Result<(), String> {
        if self.mailbox.is_none() {
            return Err("No mailbox selected".to_string());
        }
        let flags_str = if let Some(flags) = flags {
            format!("({})", flags.join(" "))
        } else {
            "()".to_string()
        };

        self.session
            .append_with_flags(self.mailbox.as_ref().unwrap(), body, flags_str)
            .map_err(|e| e.to_string())
    }

    pub fn replace_uid(&mut self, uid: u32, body: &[u8]) -> Result<(), String> {
        // Fetch the current flags so we can copy them to the new message.
        let zc_vec_fetch = self.fetch_uid_meta(uid)?;

        let mut uidres: Option<UidResult> = None;
        for fetch in zc_vec_fetch.deref() {
            if let FetchResult::Uid(res) = FetchResult::from(fetch) {
                if res.uid() == uid {
                    uidres.replace(res);
                    break;
                }
            }
        }

        if uidres.is_none() {
            return Err(format!("UID {} not found on server", uid));
        }

        // Map flags into strings. Recent is not allowed in APPEND
        let flags: Vec<String> = uidres
            .unwrap()
            .flags()
            .iter()
            .filter(|e| **e != Flag::Recent)
            .map(|flag| flag2string(flag))
            .filter(|flag| flag.is_some())
            .map(|flag| flag.unwrap())
            .collect();

        // Append first so if it fails we don't delete the original
        self.append(body, Some(&flags))?;
        self.delete_uid(uid)
    }

    pub fn add_flags_for_uid(&mut self, uid: u32, flags: &[String]) -> Result<(), String> {
        self.session
            .uid_store(format!("{}", uid), format!("+FLAGS ({})", flags.join(" ")))
            .map_err(|e| format!("STORE UID {} +FLAGS failed: {}", uid, e))
            .map(|_| ())
    }

    pub fn remove_flags_for_uid(&mut self, uid: u32, flags: &[String]) -> Result<(), String> {
        self.session
            .uid_store(format!("{}", uid), format!("-FLAGS ({})", flags.join(" ")))
            .map_err(|e| format!("STORE UID {} -FLAGS failed: {}", uid, e))
            .map(|_| ())
    }
}
