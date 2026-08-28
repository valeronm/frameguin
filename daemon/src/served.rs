//! A device on the bus, and the two things an interface impl can reach on it.
//!
//! Its own module rather than the parent of the device modules, because a
//! child module sees its parent's private fields: here the fields are
//! private to this file, so an interface impl reaches the device and the
//! polkit check only through the two methods below, and both stamp the idle
//! clock on the way.

use std::sync::Arc;

use zbus::fdo;
use zbus::message::Header;

use crate::service::Service;

pub(crate) struct Served<D> {
    device: D,
    service: Arc<Service>,
}

impl<D> Served<D> {
    pub(crate) fn new(device: D, service: Arc<Service>) -> Self {
        Self { device, service }
    }

    pub(crate) fn device(&self) -> &D {
        self.service.touch();
        &self.device
    }

    /// Call only once the arguments have been validated — see
    /// [`Service::authorize`].
    pub(crate) async fn authorize(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.service.touch();
        self.service.authorize(header).await
    }
}
