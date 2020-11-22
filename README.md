# runt

Synchronize IMAP and Maildir.

By default checks `~/.runt/config` for a toml formatted config file that specifies
one or more accounts.

A config file looks like the following:

```toml
[[accounts]]
# The account name. This is just a local identifier ("work", "home", etc.)
account = "example"

# The imap server name and port
server = "mail.example.com"
port = 993

# The account username.
username = "user"

# The password, either directly or using a program to fetch it from a password manager
# Only one of password or password_command is required
password = "accountpassword"
password_command = "pass mail.example.com"

# The path to where you want the maildir for this account
maildir = "/path/to/your/maildir"

# Optional: Path to private CA bundle if using a private certificate.
server_ca_path = "/path/to/private/ca/if/needed"

# Optional: Mailbox names to exclude from synchronization
exclude = ["Skip", "These", "Mailboxes"]

# Optional: Mailboxes to IDLE and monitor for changes.
# All mailboxes not in the `exclude` list will be synchronized on startup
# but only mailboxes in the `idle` list will be continuously monitored.
# If not present, then all synchronized mailboxes will be monitored.
idle = ["INBOX", "Other"]
```

Multiple `[[accounts]]` sections can be present to synchronize multiple IMAP
accounts.

Once the config file is set up just execute the program to synchronize the IMAP
account to local maildir. Leave the program running and it will keep the Maildir
and IMAP server in sync using IDLE and file system monitoring.

# Requirements

The server must support the `UIDPLUS`, `IDLE` and `ENABLE` capabilities.
If one of these is missing, runt will exit with an error.

If the server supports the `QRESYNC` capability, then it will be used to synchronize
quickly. Dovecot supports this capability, but Gmail does not.
