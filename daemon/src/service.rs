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

pub(crate) struct Service {
    authority: AuthorityProxy<'static>,
    last_used: Arc<Mutex<Instant>>,
}

impl Service {
    pub(crate) fn new(authority: AuthorityProxy<'static>, last_used: Arc<Mutex<Instant>>) -> Self {
        Self {
            authority,
            last_used,
        }
    }

    pub(crate) fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    /// Call only once the arguments have been validated: this can raise a
    /// password prompt, and a caller that authorizes first makes the user
    /// answer one for a request that can only end in `InvalidArgs`.
    pub(crate) async fn authorize(&self, header: &Header<'_>) -> fdo::Result<()> {
        let subject = Subject::new_for_message_header(header).map_err(internal_err)?;
        let result = self
            .authority
            .check_authorization(
                &subject,
                POLKIT_ACTION,
                &HashMap::new(),
                CheckAuthorizationFlags::AllowUserInteraction.into(),
                "",
            )
            .await
            .map_err(internal_err)?;
        if result.is_authorized {
            Ok(())
        } else {
            Err(fdo::Error::AccessDenied("not authorized".into()))
        }
    }
}
