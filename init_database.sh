#!/bin/bash

DB_FILE="sqlite3.db"

# Check for the database file and create it if it does not exist
if [ ! -f "$DB_FILE" ]; then
  echo "Database file does not exist. Creating..."
  sqlite3 "$DB_FILE" <<EOF
CREATE TABLE IF NOT EXISTS email_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id VARCHAR(255) NOT NULL,
    message_id VARCHAR(255) DEFAULT NULL,
    email VARCHAR(255) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    scheduled_at DATETIME NOT NULL,
    status TINYINT NOT NULL DEFAULT 0,
    error VARCHAR(255) DEFAULT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
    deleted_at DATETIME
);

CREATE INDEX idx_requests_status ON email_requests(status);
CREATE INDEX idx_requests_scheduled_at ON email_requests(scheduled_at DESC);
CREATE INDEX idx_requests_topic_id ON email_requests(topic_id);
CREATE INDEX idx_email_requests_message_id ON email_requests(message_id);

CREATE TABLE IF NOT EXISTS email_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id INTEGER NOT NULL,
    status VARCHAR(50) NOT NULL,
    raw TEXT,
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (request_id) REFERENCES email_requests(id)
);

CREATE INDEX idx_results_status ON email_results(status);
EOF
  echo "Database initialized."
else
  echo "Database file already exists."
fi
