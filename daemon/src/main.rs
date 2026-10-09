//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and reaches the
//! machine through `frameguin_hardware`. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

mod interface;
mod root;
mod served;
mod service;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use frameguin_hardware::device;
use zbus::Connection;

const IDLE_EXIT: Duration = Duration::from_mins(5);

fn main() -> zbus::Result<()> {
    let last_used = Arc::new(Mutex::new(Instant::now()));
    let clock = last_used.clone();
    let detected = device::detect();
    let _conn = zbus::block_on(async move {
        let conn = Connection::system().await?;
        root::serve(&conn, last_used, detected).await?;
        Ok::<_, zbus::Error>(conn)
    })?;
    loop {
        std::thread::sleep(Duration::from_mins(1));
        if clock.lock().unwrap().elapsed() > IDLE_EXIT {
            return Ok(());
        }
    }
}
