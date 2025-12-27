#!/bin/bash

DB_FILE="sqlite3.db"

# Check for the database file and create it if it does not exist
if [ ! -f "$DB_FILE" ]; then
  echo "Database file does not exist. Creating..."
  sqlite3 "$DB_FILE" <<EOF
-- 이메일 콘텐츠 테이블 (subject, content 중복 저장 방지)
CREATE TABLE IF NOT EXISTS email_contents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

-- 이메일 요청 테이블 (content_id로 콘텐츠 참조)
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

-- 개별 인덱스: 특정 조회 최적화
CREATE INDEX idx_requests_topic_id ON email_requests(topic_id);
CREATE INDEX idx_requests_content_id ON email_requests(content_id);
CREATE INDEX idx_requests_message_id ON email_requests(message_id);

-- 복합 인덱스: 스케줄러 쿼리 최적화
-- "WHERE status = ? AND scheduled_at <= datetime('now') ORDER BY scheduled_at ASC" 최적화
CREATE INDEX idx_requests_status_scheduled ON email_requests(status, scheduled_at ASC);

-- 복합 인덱스: 발송 건수 조회 최적화
-- "WHERE status = ? AND created_at >= datetime('now', ?)" 최적화
CREATE INDEX idx_requests_status_created ON email_requests(status, created_at DESC);

-- 복합 인덱스: stop_topic 쿼리 최적화
-- "WHERE status = ? AND topic_id = ?" 최적화
CREATE INDEX idx_requests_status_topic ON email_requests(status, topic_id);

CREATE TABLE IF NOT EXISTS email_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id INTEGER NOT NULL,
    status VARCHAR(50) NOT NULL,
    raw TEXT,
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (request_id) REFERENCES email_requests(id)
);

-- request_id 인덱스: JOIN/서브쿼리 최적화
CREATE INDEX idx_results_request_id ON email_results(request_id);

-- status 인덱스: 이벤트 타입별 조회 최적화
CREATE INDEX idx_results_status ON email_results(status);
EOF
  # Docker 컨테이너에서 접근 가능하도록 권한 설정
  chmod 666 "$DB_FILE"
  echo "Database initialized with permissions: $(stat -c '%a' "$DB_FILE" 2>/dev/null || stat -f '%A' "$DB_FILE")"
else
  echo "Database file already exists."
fi
