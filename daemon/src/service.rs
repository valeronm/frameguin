//! What every D-Bus object this daemon serves shares: the polkit check a
//! setter makes, and the clock the idle exit reads.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zbus::fdo;
use zbus::message::Header;
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

use crate::internal_err;

const POLKIT_ACTION: &str = "io.github.valeronm.frameguin.manage";

/// Who answers whether a caller may write: polkit, or in a test a fixed
/// answer that needs no sender to ask about.
enum Authority {
    Polkit(AuthorityProxy<'static>),
    #[cfg(test)]
    Answering(bool),
}

pub(crate) struct Service {
    authority: Authority,
    last_used: Arc<Mutex<Instant>>,
}

impl Service {
    pub(crate) fn new(authority: AuthorityProxy<'static>, last_used: Arc<Mutex<Instant>>) -> Self {
        Self {
            authority: Authority::Polkit(authority),
            last_used,
        }
    }

    #[cfg(test)]
    pub(crate) fn answering(authorized: bool) -> Self {
        Self {
            authority: Authority::Answering(authorized),
            last_used: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub(crate) fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    /// Call only once the arguments have been validated: this can raise a
    /// password prompt, and a caller that authorizes first makes the user
    /// answer one for a request that can only end in `InvalidArgs`.
    pub(crate) async fn authorize(&self, header: &Header<'_>) -> fdo::Result<()> {
        let authorized = match &self.authority {
            Authority::Polkit(authority) => polkit_authorizes(authority, header).await?,
            #[cfg(test)]
            Authority::Answering(authorized) => *authorized,
        };
        if authorized {
            Ok(())
        } else {
            Err(fdo::Error::AccessDenied("not authorized".into()))
        }
    }
}

async fn polkit_authorizes(
    authority: &AuthorityProxy<'static>,
    header: &Header<'_>,
) -> fdo::Result<bool> {
    let subject = Subject::new_for_message_header(header).map_err(internal_err)?;
    let result = authority
        .check_authorization(
            &subject,
            POLKIT_ACTION,
            &HashMap::new(),
            CheckAuthorizationFlags::AllowUserInteraction.into(),
            "",
        )
        .await
        .map_err(internal_err)?;
    Ok(result.is_authorized)
}
