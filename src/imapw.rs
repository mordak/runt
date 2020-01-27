use config::Config;
use imap::types::{Fetch, Mailbox, Name, ZeroCopy};
use imap::Client;
use imap::Session as SubSession;
use imap_proto::types::Capability;
use native_tls::TlsConnector;
use native_tls::TlsStream;
use std::net::TcpStream;
use std::ops::Deref;
use std::time::Duration;
use std::vec::Vec;

pub struct Session {
    session: SubSession<TlsStream<TcpStream>>,
}

impl Session {
    pub fn new(config: &Config) -> Result<Session, String> {
        let client = Session::connect(config)?;
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

        Ok(Session { session })
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
                i.set_keepalive(Duration::from_secs(5 * 60));
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
            .uid_fetch(range, "(UID FLAGS INTERNALDATE RFC822.SIZE)")
            .map_err(|e| format!("{}", e))
    }

    pub fn select_mailbox(&mut self, mailbox: &str) -> Result<Mailbox, String> {
        self.debug(true);
        self.session
            .select(mailbox)
            .map_err(|e| format!("SELECT {} failed: {}", mailbox, e))
    }

    pub fn logout(&mut self) -> Result<(), String> {
        self.session
            .logout()
            .map_err(|e| format!("LOGOUT failed: {}", e))
    }
}
