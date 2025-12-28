//! Common constants used across the application

/// Max records per batch INSERT (`SQLite` variable limit: 999)
pub const BATCH_INSERT_SIZE: usize = 100;
