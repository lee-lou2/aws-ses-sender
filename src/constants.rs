//! Common constants used across the application

/// Max records per batch INSERT (`SQLite` variable limit: 999).
/// With 5 columns per row, 150 rows = 750 placeholders (safe margin).
pub const BATCH_INSERT_SIZE: usize = 150;
