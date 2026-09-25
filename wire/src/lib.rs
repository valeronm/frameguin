//! The transport of `io.github.valeronm.Frameguin1`: the name and path the
//! daemon serves at, a proxy per interface, how an error crosses, and
//! [`Bus`]. The daemon's `#[interface]` impls restate the same methods, and
//! meet these proxies in the daemon's own tests.

mod bus;
mod error;
mod proxies;

pub use bus::*;
pub use error::*;
pub use proxies::*;

pub const BUS_NAME: &str = "io.github.valeronm.Frameguin";
pub const OBJECT_PATH: &str = "/io/github/valeronm/Frameguin";
