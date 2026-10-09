//! A daemon answering as another machine would, for looking at the app on
//! hardware this one is not: the real devices over stub roles, behind the
//! interfaces the real daemon serves.
//!
//! Answers for polkit as well, with one fixed answer, so it runs only on a
//! private bus named by `DBUS_SYSTEM_BUS_ADDRESS`.

mod interface;
#[path = "fake/profile.rs"]
mod profile;
mod root;
mod served;
mod service;

use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zbus::Connection;

use crate::polkit::Polkit;

const BUS_ADDRESS: &str = "DBUS_SYSTEM_BUS_ADDRESS";
const POLKIT_NAME: &str = "org.freedesktop.PolicyKit1";
const POLKIT_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";

mod polkit {
    #![expect(
        clippy::used_underscore_binding,
        reason = "the arguments are polkit's, decoded by the interface macro and read by nothing"
    )]

    use std::collections::HashMap;

    use zbus::interface;
    use zbus_polkit::policykit1::{AuthorizationResult, Subject};

    pub(crate) struct Polkit {
        pub(crate) authorized: bool,
    }

    #[interface(name = "org.freedesktop.PolicyKit1.Authority")]
    impl Polkit {
        fn check_authorization(
            &self,
            _subject: Subject,
            _action_id: &str,
            _details: HashMap<String, String>,
            _flags: u32,
            _cancellation_id: &str,
        ) -> AuthorizationResult {
            AuthorizationResult {
                is_authorized: self.authorized,
                is_challenge: false,
                details: HashMap::new(),
            }
        }
    }
}

struct Args {
    board: String,
    authorized: bool,
    refusing_writes: bool,
}

fn parse(args: impl Iterator<Item = String>) -> Option<Args> {
    let mut board = None;
    let mut authorized = true;
    let mut refusing_writes = false;
    for arg in args {
        match arg.as_str() {
            "--deny" => authorized = false,
            "--refuse-writes" => refusing_writes = true,
            name if !name.starts_with('-') && board.is_none() => board = Some(arg),
            _ => return None,
        }
    }
    Some(Args {
        board: board?,
        authorized,
        refusing_writes,
    })
}

fn usage() -> ExitCode {
    eprintln!("usage: frameguin-fake-daemon <board> [--deny] [--refuse-writes]");
    eprintln!("  --deny            polkit refuses every write");
    eprintln!("  --refuse-writes   the hardware refuses every write it can");
    eprintln!("boards:");
    for name in profile::names() {
        eprintln!("  {name}");
    }
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let Some(args) = parse(std::env::args().skip(1)) else {
        return usage();
    };
    let Some(detected) = profile::detected(&args.board, args.refusing_writes) else {
        return usage();
    };
    if std::env::var_os(BUS_ADDRESS).is_none() {
        eprintln!("{BUS_ADDRESS} is not set: the fake daemon runs only on a private bus");
        return ExitCode::from(2);
    }
    let polkit = Polkit {
        authorized: args.authorized,
    };
    let served = zbus::block_on(async move {
        let conn = Connection::system().await?;
        conn.object_server().at(POLKIT_PATH, polkit).await?;
        // A bus that already has a polkit refuses this name, which is what
        // keeps the fake off a real system bus.
        conn.request_name(POLKIT_NAME).await?;
        root::serve(&conn, Arc::new(Mutex::new(Instant::now())), detected).await?;
        Ok::<_, zbus::Error>(conn)
    });
    match served {
        Ok(_conn) => loop {
            std::thread::park();
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
