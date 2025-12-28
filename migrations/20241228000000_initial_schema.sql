-- Initial schema for aws-ses-sender

-- Email contents table (prevents duplicate subject/content storage)
CREATE TABLE IF NOT EXISTS email_contents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

-- Email requests table (references content via content_id)
CREATE TABLE IF NOT EXISTS email_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id VARCHAR(255) NOT NULL,
    content_id INTEGER NOT NULL,
    message_id VARCHAR(255) DEFAULT NULL,
    email VARCHAR(255) NOT NULL,
    scheduled_at DATETIME NOT NULL,
    status TINYINT NOT NULL DEFAULT 0,
    error VARCHAR(255) DEFAULT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
    deleted_at DATETIME,
    FOREIGN KEY (content_id) REFERENCES email_contents(id)
);

-- Individual indexes
CREATE INDEX IF NOT EXISTS idx_requests_topic_id ON email_requests(topic_id);
CREATE INDEX IF NOT EXISTS idx_requests_content_id ON email_requests(content_id);
CREATE INDEX IF NOT EXISTS idx_requests_message_id ON email_requests(message_id);

-- Composite index: scheduler query optimization
CREATE INDEX IF NOT EXISTS idx_requests_status_scheduled ON email_requests(status, scheduled_at ASC);

-- Composite index: sent count query optimization
CREATE INDEX IF NOT EXISTS idx_requests_status_created ON email_requests(status, created_at DESC);

-- Composite index: stop_topic query optimization
CREATE INDEX IF NOT EXISTS idx_requests_status_topic ON email_requests(status, topic_id);

-- Email results table
CREATE TABLE IF NOT EXISTS email_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id INTEGER NOT NULL,
    status VARCHAR(50) NOT NULL,
    raw TEXT,
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (request_id) REFERENCES email_requests(id)
);

-- Result indexes
CREATE INDEX IF NOT EXISTS idx_results_request_id ON email_results(request_id);
CREATE INDEX IF NOT EXISTS idx_results_status ON email_results(status);
