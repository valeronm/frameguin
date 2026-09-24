//! A device on the bus, and the two ways an interface impl reaches it: to
//! read, and once authorized, to write.
//!
//! Its own module rather than the parent of the device modules, because a
//! child module sees its parent's private fields: here the fields are
//! private to this file, so an interface impl reaches the device only
//! through the two methods below.

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

    /// The device, once the caller is authorized to write to it.
    pub(crate) async fn authorized(&self, header: &Header<'_>) -> fdo::Result<&D> {
        self.service.authorize(header).await?;
        Ok(&self.device)
    }
}
