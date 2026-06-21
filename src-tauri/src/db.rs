//! ALAN Echo — SQLite transcript database with FTS5, pagination, backup, export.

use chrono::{Local, SecondsFormat};
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transcript {
    pub id: i64,
    pub timestamp: String,
    pub text: String,
    pub raw_text: Option<String>,
    pub duration_seconds: Option<f64>,
    pub word_count: Option<i32>,
    pub character_count: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    pub total: i64,
    pub words: i64,
    pub duration: f64,
}

pub struct TranscriptDB {
    conn: Connection,
    path: std::path::PathBuf,
}

impl TranscriptDB {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transcripts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                text TEXT NOT NULL,
                raw_text TEXT,
                duration_seconds REAL,
                character_count INTEGER,
                word_count INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_timestamp ON transcripts(timestamp DESC);
            CREATE TABLE IF NOT EXISTS schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );"
        )?;

        // FTS5
        let has_fts: bool = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE name='transcripts_fts'")?
            .exists([])?;

        if !has_fts {
            conn.execute_batch(
                "CREATE VIRTUAL TABLE transcripts_fts USING fts5(text, content=transcripts, content_rowid=id);

                CREATE TRIGGER IF NOT EXISTS trg_fts_insert AFTER INSERT ON transcripts BEGIN
                    INSERT INTO transcripts_fts(rowid, text) VALUES (new.id, new.text);
                END;
                CREATE TRIGGER IF NOT EXISTS trg_fts_delete AFTER DELETE ON transcripts BEGIN
                    INSERT INTO transcripts_fts(transcripts_fts, rowid, text) VALUES('delete', old.id, old.text);
                END;
                CREATE TRIGGER IF NOT EXISTS trg_fts_update AFTER UPDATE OF text ON transcripts BEGIN
                    INSERT INTO transcripts_fts(transcripts_fts, rowid, text) VALUES('delete', old.id, old.text);
                    INSERT INTO transcripts_fts(rowid, text) VALUES (new.id, new.text);
                END;

                INSERT INTO transcripts_fts(transcripts_fts) VALUES('rebuild');"
            )?;
        }

        // Migration: add raw_text if missing
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(transcripts)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !columns.contains(&"raw_text".to_string()) {
            conn.execute("ALTER TABLE transcripts ADD COLUMN raw_text TEXT", [])?;
            conn.execute("UPDATE transcripts SET raw_text = text WHERE raw_text IS NULL", [])?;
        }

        Ok(Self { conn, path: path.to_path_buf() })
    }

    pub fn save(&self, text: &str, raw_text: Option<&str>, duration: f64) -> Result<i64> {
        // RFC3339 with UTC offset (e.g. 2026-06-10T15:29:00.123456-06:00) — an
        // offset-less local string is ambiguous across DST/timezone changes.
        // Same prefix shape as legacy rows, so lexicographic ORDER BY still sorts
        // chronologically against them.
        let ts = Local::now().to_rfc3339_opts(SecondsFormat::Micros, false);
        let wc = text.split_whitespace().count() as i32;
        let cc = text.chars().count() as i32;
        self.conn.execute(
            "INSERT INTO transcripts (timestamp, text, raw_text, duration_seconds, character_count, word_count) VALUES (?1,?2,?3,?4,?5,?6)",
            params![ts, text, raw_text.unwrap_or(text), duration, cc, wc],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_page(&self, page: u32, page_size: u32) -> Result<(Vec<Transcript>, i64)> {
        let total: i64 = self.conn.query_row("SELECT COUNT(*) FROM transcripts", [], |r| r.get(0))?;
        let offset = page * page_size;
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, text, raw_text, duration_seconds, character_count, word_count FROM transcripts ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2"
        )?;
        let rows = stmt.query_map(params![page_size, offset], |row| {
            Ok(Transcript {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                text: row.get(2)?,
                raw_text: row.get(3)?,
                duration_seconds: row.get(4)?,
                character_count: row.get(5)?,
                word_count: row.get(6)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok((rows, total))
    }

    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<Transcript>> {
        if query.trim().is_empty() {
            return self.get_page(0, limit).map(|(t, _)| t);
        }
        // Try FTS5 first (wrap entire attempt so query errors also fall back)
        let fts_result: Result<Vec<Transcript>> = (|| {
            let mut stmt = self.conn.prepare(
                "SELECT t.id, t.timestamp, t.text, t.raw_text, t.duration_seconds, t.character_count, t.word_count
                 FROM transcripts_fts fts JOIN transcripts t ON t.id = fts.rowid
                 WHERE transcripts_fts MATCH ?1 ORDER BY rank LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![query, limit], |row| {
                Ok(Transcript {
                    id: row.get(0)?, timestamp: row.get(1)?, text: row.get(2)?,
                    raw_text: row.get(3)?, duration_seconds: row.get(4)?,
                    character_count: row.get(5)?, word_count: row.get(6)?,
                })
            })?.filter_map(|r| r.ok()).collect();
            Ok(rows)
        })();
        match fts_result {
            Ok(rows) => Ok(rows),
            Err(_) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, timestamp, text, raw_text, duration_seconds, character_count, word_count FROM transcripts WHERE text LIKE ?1 ORDER BY timestamp DESC LIMIT ?2"
                )?;
                let pattern = format!("%{}%", query);
                let rows = stmt.query_map(params![pattern, limit], |row| {
                    Ok(Transcript {
                        id: row.get(0)?, timestamp: row.get(1)?, text: row.get(2)?,
                        raw_text: row.get(3)?, duration_seconds: row.get(4)?,
                        character_count: row.get(5)?, word_count: row.get(6)?,
                    })
                })?.filter_map(|r| r.ok()).collect();
                Ok(rows)
            }
        }
    }

    pub fn get_stats(&self) -> Result<Stats> {
        self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(word_count),0), COALESCE(SUM(duration_seconds),0.0) FROM transcripts",
            [],
            |row| Ok(Stats { total: row.get(0)?, words: row.get(1)?, duration: row.get(2)? }),
        )
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        let affected = self.conn.execute("DELETE FROM transcripts WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    pub fn update_text(&self, id: i64, text: &str) -> Result<bool> {
        let wc = text.split_whitespace().count() as i32;
        let cc = text.chars().count() as i32;
        let affected = self.conn.execute(
            "UPDATE transcripts SET text=?1, word_count=?2, character_count=?3 WHERE id=?4",
            params![text, wc, cc, id],
        )?;
        Ok(affected > 0)
    }

    pub fn backup(&self) -> Result<String> {
        let backup_dir = self.path.parent().unwrap_or(std::path::Path::new(".")).join("backups");
        std::fs::create_dir_all(&backup_dir).ok();
        let ts = Local::now().format("%Y%m%d_%H%M%S");
        let backup_path = backup_dir.join(format!("transcripts_{}.db", ts));
        // Simple file copy for backup (SQLite WAL mode is safe to copy after checkpoint)
        self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        std::fs::copy(&self.path, &backup_path)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        prune_backups(&backup_dir, 7);
        Ok(backup_path.to_string_lossy().to_string())
    }

    pub fn maybe_daily_backup(&self) {
        let backup_dir = self.path.parent().unwrap_or(std::path::Path::new(".")).join("backups");
        let today = Local::now().format("%Y%m%d").to_string();
        if backup_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&backup_dir) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().contains(&today) {
                        return; // Already backed up today
                    }
                }
            }
        }
        let _ = self.backup();
    }

    pub fn export(&self, path: &str, format: &str) -> Result<bool> {
        let transcripts = self.get_page(0, 999999).map(|(t, _)| t)?;
        let reversed: Vec<_> = transcripts.into_iter().rev().collect();

        match format {
            "json" => {
                let json = serde_json::to_string_pretty(&reversed)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                std::fs::write(path, json).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            }
            "csv" => {
                // Quote-escape AND neutralize spreadsheet formula injection: a
                // cell beginning with = + - @ is evaluated as a formula by Excel
                // / Sheets even inside quotes (a dictated transcript starting
                // with one of those could exfiltrate data via WEBSERVICE/DDE),
                // so prefix those cells with an apostrophe.
                let cell = |s: &str| -> String {
                    let cleaned = s.replace('"', "\"\"").replace('\n', " ").replace('\r', "");
                    if cleaned.starts_with(|c| matches!(c, '=' | '+' | '-' | '@')) {
                        format!("'{}", cleaned)
                    } else {
                        cleaned
                    }
                };
                let mut out = String::from("Timestamp,Text,Duration,Words\n");
                for t in &reversed {
                    out.push_str(&format!(
                        "\"{}\",\"{}\",{},{}\n",
                        t.timestamp.get(..19).unwrap_or(&t.timestamp).replace('T', " "),
                        cell(&t.text),
                        t.duration_seconds.unwrap_or(0.0),
                        t.word_count.unwrap_or(0),
                    ));
                }
                std::fs::write(path, out).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            }
            "md" => {
                let mut out = String::from("# ALAN Echo — Transcription Export\n\n---\n\n");
                for t in &reversed {
                    out.push_str(&format!(
                        "### {} ({:.0}s, {} words)\n\n{}\n\n---\n\n",
                        t.timestamp.get(..19).unwrap_or(&t.timestamp).replace('T', " "),
                        t.duration_seconds.unwrap_or(0.0),
                        t.word_count.unwrap_or(0),
                        t.text,
                    ));
                }
                std::fs::write(path, out).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            }
            _ => {
                // txt default
                let mut out = String::new();
                for t in &reversed {
                    out.push_str(&format!("[{}]\n{}\n\n", t.timestamp.get(..19).unwrap_or(&t.timestamp).replace('T', " "), t.text));
                }
                std::fs::write(path, out).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            }
        }
        Ok(true)
    }
}

/// Keep only the newest `keep` backups. Timestamped filenames sort
/// chronologically, so a name sort is a date sort.
fn prune_backups(backup_dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(backup_dir) else { return };
    let mut backups: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "db").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("transcripts_"))
                    .unwrap_or(false)
        })
        .collect();
    if backups.len() <= keep {
        return;
    }
    backups.sort();
    let excess = backups.len() - keep;
    for old in backups.into_iter().take(excess) {
        std::fs::remove_file(old).ok();
    }
}
