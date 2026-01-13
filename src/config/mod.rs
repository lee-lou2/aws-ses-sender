//! Configuration module.

mod db;
mod env;

pub use db::{close_db, init_db};
pub use env::APP_CONFIG;
