//! 설정 모듈.

mod db;
mod env;

pub use db::{close_db, init_db};
pub use env::APP_CONFIG;
