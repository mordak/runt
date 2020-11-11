# runt

Synchronize IMAP and Maildir.

By default checks `~/.runt/config` for a toml formatted config file that specifies
one or more accounts.

A config file looks like the following:

```toml
[[accounts]]
# The account name
account = "example"
# The imap server name and port
server = "mail.example.com"
port = 993
# The account username
username = "accountname"
# The password, either directly or using a program to fetch it from a password manager
# Only one of password or password_command is required
password = "accountpassword"
password_command = "pass mail.example.com"
# The path to where you want the maildir for this account
maildir = "/path/to/your/maildir"
# Optional path to private CA bundle
server_ca_path = "/path/to/private/ca/if/needed"
# Mailbox names to exclude from synchronization
exclude = ["Skip", "These", "Mailboxes"]
```

Once the config file is set up just execute the program to synchronize the IMAP
account to local maildir. Leave the program running and it will keep the Maildir
and IMAP server in sync.

# Requirements

The server must support the `UIDPLUS`, `IDLE`, `ENABLE`, and `QRESYNC` capabilities.
If one of these is missing, runt will exit with an error.
