//! Session recording in SQLite (`pulse.db` in the data folder).
//!
//! A session starts with the first sample while none runs and ends after
//! `idleTimeoutSecs` without one. Samples are buffered and written at most a
//! second later, in one transaction together with the session's running
//! min/max/avg, so a busy strap costs one write per second, not one per beat.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::types::{ExportFormat, HrSample, Recording, SessionDetail, SessionSummary};
use crate::Error;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    min_bpm INTEGER NOT NULL,
    max_bpm INTEGER NOT NULL,
    avg_bpm REAL NOT NULL,
    sample_count INTEGER NOT NULL,
    sources TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS samples (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    bpm INTEGER NOT NULL,
    src TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS samples_session_ts ON samples(session_id, ts);
";

/// Buffered samples are written once the oldest is this old.
const FLUSH_AFTER: Duration = Duration::from_secs(1);

struct Active {
    id: String,
    started_at: i64,
    last_ts: i64,
    last_at: Instant,
    buffer: Vec<(i64, u16, &'static str)>,
    buffered_since: Option<Instant>,
    min: u16,
    max: u16,
    sum: u64,
    count: u64,
    sources: BTreeSet<&'static str>,
}

impl Active {
    fn summary(&self) -> SessionSummary {
        SessionSummary {
            id: self.id.clone(),
            started_at: self.started_at,
            ended_at: None,
            min_bpm: self.min,
            max_bpm: self.max,
            avg_bpm: avg(self.sum, self.count),
            sample_count: self.count,
            sources: self.sources.iter().copied().collect::<Vec<_>>().join(","),
        }
    }
}

fn avg(sum: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        // One decimal is all anyone reads.
        ((sum as f64 / count as f64) * 10.0).round() / 10.0
    }
}

struct Inner {
    db: Connection,
    active: Option<Active>,
}

pub(crate) struct Recorder {
    inner: Mutex<Inner>,
}

/// What a push changed, for the hub's events.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Pushed {
    pub started: bool,
}

impl Recorder {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        db.execute_batch(SCHEMA)?;
        // Sessions a crash or kill left open end at their last sample.
        db.execute(
            "UPDATE sessions SET ended_at = COALESCE(
                (SELECT MAX(ts) FROM samples WHERE session_id = sessions.id), started_at)
             WHERE ended_at IS NULL",
            [],
        )?;
        Ok(Self {
            inner: Mutex::new(Inner { db, active: None }),
        })
    }

    pub fn push(&self, s: &HrSample) -> Result<Pushed, Error> {
        let mut g = self.inner.lock();
        let mut pushed = Pushed::default();
        let src = s.src.as_str();
        if g.active.is_none() {
            let id = uuid::Uuid::new_v4().to_string();
            g.db.execute(
                "INSERT INTO sessions (id, started_at, ended_at, min_bpm, max_bpm, avg_bpm, sample_count, sources)
                 VALUES (?1, ?2, NULL, ?3, ?3, ?3, 0, ?4)",
                params![id, s.ts, s.bpm, src],
            )?;
            g.active = Some(Active {
                id,
                started_at: s.ts,
                last_ts: s.ts,
                last_at: Instant::now(),
                buffer: Vec::new(),
                buffered_since: None,
                min: s.bpm,
                max: s.bpm,
                sum: 0,
                count: 0,
                sources: BTreeSet::new(),
            });
            pushed.started = true;
        }
        let a = g.active.as_mut().expect("set above");
        a.last_ts = a.last_ts.max(s.ts);
        a.last_at = Instant::now();
        a.buffer.push((s.ts, s.bpm, src));
        a.buffered_since.get_or_insert_with(Instant::now);
        a.min = a.min.min(s.bpm);
        a.max = a.max.max(s.bpm);
        a.sum += s.bpm as u64;
        a.count += 1;
        a.sources.insert(src);
        if a.buffered_since.is_some_and(|t| t.elapsed() >= FLUSH_AFTER) {
            flush(&mut g)?;
        }
        Ok(pushed)
    }

    /// Writes what's buffered and ends the session if it went quiet. Returns
    /// whether a session ended.
    pub fn tick(&self, idle: Duration) -> Result<bool, Error> {
        let mut g = self.inner.lock();
        flush(&mut g)?;
        if g.active
            .as_ref()
            .is_some_and(|a| a.last_at.elapsed() >= idle)
        {
            end(&mut g)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn flush(&self) -> Result<(), Error> {
        flush(&mut self.inner.lock())
    }

    /// Ends the running session now (recording turned off, app quitting).
    pub fn end(&self) -> Result<bool, Error> {
        let mut g = self.inner.lock();
        if g.active.is_none() {
            return Ok(false);
        }
        end(&mut g)?;
        Ok(true)
    }

    pub fn recording(&self) -> Option<Recording> {
        self.inner.lock().active.as_ref().map(|a| Recording {
            session_id: a.id.clone(),
            started_at: a.started_at,
        })
    }

    pub fn list(&self) -> Result<Vec<SessionSummary>, Error> {
        let mut g = self.inner.lock();
        flush(&mut g)?;
        let mut stmt = g.db.prepare(
            "SELECT id, started_at, ended_at, min_bpm, max_bpm, avg_bpm, sample_count, sources
             FROM sessions ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map([], summary_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get(&self, id: &str) -> Result<SessionDetail, Error> {
        let mut g = self.inner.lock();
        flush(&mut g)?;
        let summary = match g.active.as_ref().filter(|a| a.id == id) {
            Some(a) => a.summary(),
            None => g
                .db
                .query_row(
                    "SELECT id, started_at, ended_at, min_bpm, max_bpm, avg_bpm, sample_count, sources
                     FROM sessions WHERE id = ?1",
                    [id],
                    summary_row,
                )
                .optional()?
                .ok_or(Error::NotFound)?,
        };
        let mut stmt =
            g.db.prepare("SELECT ts, bpm FROM samples WHERE session_id = ?1 ORDER BY ts, rowid")?;
        let samples = stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        Ok(SessionDetail { summary, samples })
    }

    /// The samples with their source, for the CSV.
    fn samples_with_src(&self, id: &str) -> Result<Vec<(i64, u16, String)>, Error> {
        let g = self.inner.lock();
        let mut stmt = g
            .db
            .prepare("SELECT ts, bpm, src FROM samples WHERE session_id = ?1 ORDER BY ts, rowid")?;
        let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn delete(&self, id: &str) -> Result<(), Error> {
        let mut g = self.inner.lock();
        if g.active.as_ref().is_some_and(|a| a.id == id) {
            // Deleting the running session: drop its buffer, the next sample
            // starts a new one.
            g.active = None;
        }
        let tx = g.db.transaction()?;
        tx.execute("DELETE FROM samples WHERE session_id = ?1", [id])?;
        let n = tx.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        tx.commit()?;
        if n == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    pub fn export(&self, id: &str, format: ExportFormat) -> Result<Vec<u8>, Error> {
        let detail = self.get(id)?;
        Ok(match format {
            ExportFormat::Json => {
                serde_json::to_vec_pretty(&detail).map_err(std::io::Error::other)?
            }
            ExportFormat::Csv => {
                let mut out = String::from("timestamp_ms,iso8601,bpm,source\n");
                for (ts, bpm, src) in self.samples_with_src(id)? {
                    out.push_str(&format!("{ts},{},{bpm},{src}\n", iso8601(ts)));
                }
                out.into_bytes()
            }
        })
    }
}

fn summary_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: r.get(0)?,
        started_at: r.get(1)?,
        ended_at: r.get(2)?,
        min_bpm: r.get(3)?,
        max_bpm: r.get(4)?,
        avg_bpm: r.get(5)?,
        sample_count: r.get::<_, i64>(6)?.max(0) as u64,
        sources: r.get(7)?,
    })
}

fn flush(g: &mut Inner) -> Result<(), Error> {
    let Inner { db, active } = g;
    let Some(a) = active.as_mut() else {
        return Ok(());
    };
    if a.buffer.is_empty() {
        return Ok(());
    }
    let tx = db.transaction()?;
    {
        let mut ins = tx.prepare_cached(
            "INSERT INTO samples (session_id, ts, bpm, src) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (ts, bpm, src) in &a.buffer {
            ins.execute(params![a.id, ts, bpm, src])?;
        }
    }
    let s = a.summary();
    tx.execute(
        "UPDATE sessions SET min_bpm = ?2, max_bpm = ?3, avg_bpm = ?4, sample_count = ?5, sources = ?6
         WHERE id = ?1",
        params![s.id, s.min_bpm, s.max_bpm, s.avg_bpm, s.sample_count as i64, s.sources],
    )?;
    tx.commit()?;
    a.buffer.clear();
    a.buffered_since = None;
    Ok(())
}

fn end(g: &mut Inner) -> Result<(), Error> {
    flush(g)?;
    if let Some(a) = g.active.take() {
        g.db.execute(
            "UPDATE sessions SET ended_at = ?2 WHERE id = ?1",
            params![a.id, a.last_ts],
        )?;
    }
    Ok(())
}

/// Unix milliseconds as `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC).
pub fn iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let milli = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{milli:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Src;

    #[test]
    fn iso() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601(1_767_225_600_123), "2026-01-01T00:00:00.123Z");
        assert_eq!(iso8601(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso8601(-1), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn records_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulse.db");
        let r = Recorder::open(&path).unwrap();
        let s = |ts, bpm, src| HrSample {
            ts,
            bpm,
            src,
            dev: "x".into(),
        };
        assert!(r.push(&s(1000, 70, Src::Watch)).unwrap().started);
        assert!(!r.push(&s(2000, 90, Src::Ble)).unwrap().started);
        let d = r.get(&r.recording().unwrap().session_id).unwrap();
        assert_eq!(d.samples, vec![(1000, 70), (2000, 90)]);
        assert_eq!(d.summary.avg_bpm, 80.0);
        assert_eq!(d.summary.sources, "ble,watch");
        drop(r);
        // Reopening closes the session the "crash" left open.
        let r = Recorder::open(&path).unwrap();
        let list = r.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].ended_at, Some(2000));
        assert_eq!(list[0].sample_count, 2);
        let csv = String::from_utf8(r.export(&list[0].id, ExportFormat::Csv).unwrap()).unwrap();
        assert_eq!(
            csv,
            "timestamp_ms,iso8601,bpm,source\n1000,1970-01-01T00:00:01.000Z,70,watch\n2000,1970-01-01T00:00:02.000Z,90,ble\n"
        );
        r.delete(&list[0].id).unwrap();
        assert!(matches!(r.delete(&list[0].id), Err(Error::NotFound)));
        assert!(r.list().unwrap().is_empty());
    }
}
