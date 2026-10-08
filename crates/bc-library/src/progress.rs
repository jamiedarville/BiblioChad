use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use bc_core::{Locator, ReadStatus};
use bc_sync::{BookEntry, ResumePrompt};

use crate::{Library, Result};

#[derive(Debug, Clone, Serialize)]
pub struct ProgressRecord {
    pub device_id: String,
    pub locator: Locator,
    pub percent: f64,
    pub status: ReadStatus,
    pub updated_at: i64,
}

impl ProgressRecord {
    fn entry(&self) -> BookEntry {
        BookEntry {
            locator: self.locator.clone(),
            percent: self.percent,
            status: self.status,
            updated_at: self.updated_at,
            device_id: self.device_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub total_books: i64,
    pub finished: i64,
    pub reading: i64,
    pub streak_days: i64,
    pub rank: bc_core::rank::Rank,
}

impl Library {
    /// Save this device's position. `day` is the local calendar day
    /// (YYYY-MM-DD) used for streaks.
    pub fn save_progress(
        &self,
        book_id: i64,
        device_id: &str,
        locator: &Locator,
        percent: f64,
        day: Option<&str>,
    ) -> Result<ProgressRecord> {
        let percent = percent.clamp(0.0, 1.0);
        let status = ReadStatus::from_percent(percent).max_reading();
        let now = bc_core::now_ms();
        let c = self.conn();
        c.execute(
            "INSERT INTO progress(book_id, device_id, locator_json, percent, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(book_id, device_id) DO UPDATE SET
               locator_json = excluded.locator_json, percent = excluded.percent,
               status = excluded.status, updated_at = excluded.updated_at",
            params![
                book_id,
                device_id,
                locator.to_json(),
                percent,
                status.as_str(),
                now
            ],
        )?;
        // Reading again supersedes a manual "unread"; "finished" stays until
        // the reader actually reaches the end again or resets it.
        c.execute(
            "UPDATE books SET status_override = NULL WHERE id = ?1 AND status_override = 'unread'",
            [book_id],
        )?;
        if let Some(day) = day {
            c.execute("INSERT OR IGNORE INTO reading_days(day) VALUES (?1)", [day])?;
        }
        Ok(ProgressRecord {
            device_id: device_id.into(),
            locator: locator.clone(),
            percent,
            status,
            updated_at: now,
        })
    }

    /// Insert a position reported by another device (from sync). Ignored if
    /// we already hold a newer row for that device.
    pub fn upsert_remote_progress(&self, book_id: i64, e: &BookEntry) -> Result<()> {
        self.conn().execute(
            "INSERT INTO progress(book_id, device_id, locator_json, percent, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(book_id, device_id) DO UPDATE SET
               locator_json = excluded.locator_json, percent = excluded.percent,
               status = excluded.status, updated_at = excluded.updated_at
             WHERE excluded.updated_at > progress.updated_at",
            params![
                book_id,
                e.device_id,
                e.locator.to_json(),
                e.percent,
                e.status.as_str(),
                e.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn progress_rows(&self, book_id: i64) -> Result<Vec<ProgressRecord>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT device_id, locator_json, percent, status, updated_at FROM progress
             WHERE book_id = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = st.query_map([book_id], |r| {
            let loc: String = r.get(1)?;
            let status: String = r.get(3)?;
            Ok((
                r.get::<_, String>(0)?,
                loc,
                r.get::<_, f64>(2)?,
                status,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (device_id, loc, percent, status, updated_at) = row?;
            let Ok(locator) = Locator::from_json(&loc) else {
                continue;
            };
            out.push(ProgressRecord {
                device_id,
                locator,
                percent,
                status: ReadStatus::parse(&status).unwrap_or_default(),
                updated_at,
            });
        }
        Ok(out)
    }

    pub fn progress_for(&self, book_id: i64, device_id: &str) -> Result<Option<ProgressRecord>> {
        Ok(self
            .progress_rows(book_id)?
            .into_iter()
            .find(|p| p.device_id == device_id))
    }

    /// Where to open a book, plus an optional prompt if another device got
    /// meaningfully further more recently.
    pub fn resume_point(
        &self,
        book_id: i64,
        device_id: &str,
    ) -> Result<(Option<ProgressRecord>, Option<ResumePrompt>)> {
        let rows = self.progress_rows(book_id)?;
        let mine = rows.iter().find(|p| p.device_id == device_id).cloned();
        let newest_other = rows.iter().find(|p| p.device_id != device_id);
        let prompt = newest_other.and_then(|o| {
            bc_sync::resume_prompt(
                mine.as_ref().map(|m| m.entry()).as_ref(),
                &o.entry(),
                device_id,
            )
        });
        // With no local row at all, just start where the other device is.
        if mine.is_none() {
            if let Some(o) = newest_other {
                return Ok((Some(o.clone()), None));
            }
        }
        Ok((mine, prompt))
    }

    /// Reset a book to unread and forget all positions.
    pub fn reset_progress(&self, book_id: i64) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM progress WHERE book_id = ?1", [book_id])?;
        c.execute(
            "UPDATE books SET status_override = NULL WHERE id = ?1",
            [book_id],
        )?;
        Ok(())
    }

    /// Counts for the stats card. `today` is the local day (YYYY-MM-DD).
    pub fn stats(&self, today: &str) -> Result<Stats> {
        let (total, finished, reading) = {
            let c = self.conn();
            c.query_row(
                "SELECT COUNT(*),
                        SUM(COALESCE(b.status_override, p.status) = 'finished'),
                        SUM(COALESCE(b.status_override, p.status) = 'reading')
                 FROM books b
                 LEFT JOIN (SELECT book_id, status,
                              ROW_NUMBER() OVER (PARTITION BY book_id ORDER BY updated_at DESC) rn
                            FROM progress) p ON p.book_id = b.id AND p.rn = 1
                 WHERE EXISTS(SELECT 1 FROM nodes n WHERE n.file_key = b.book_key AND n.is_trashed = 0)",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    ))
                },
            )?
        };
        Ok(Stats {
            total_books: total,
            finished,
            reading,
            streak_days: self.streak(today)?,
            rank: bc_core::rank::rank_for(finished as u32),
        })
    }

    fn streak(&self, today: &str) -> Result<i64> {
        // Count consecutive days ending today (or yesterday, so a streak is
        // not "broken" before you have had a chance to read today).
        let c = self.conn();
        let mut st = c.prepare("SELECT day FROM reading_days ORDER BY day DESC LIMIT 400")?;
        let days: Vec<String> = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let parse = |s: &str| chrono_like::days_from_civil(s);
        let Some(t) = parse(today) else { return Ok(0) };
        let mut expected = t;
        let mut streak = 0;
        for (i, d) in days.iter().enumerate() {
            let Some(d) = parse(d) else { continue };
            if i == 0 && d == t - 1 {
                expected = t - 1;
            }
            if d == expected {
                streak += 1;
                expected -= 1;
            } else if d < expected {
                break;
            }
        }
        Ok(streak)
    }

    pub fn last_progress_update(&self) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row("SELECT MAX(updated_at) FROM progress", [], |r| r.get(0))
            .optional()?
            .flatten())
    }
}

trait MaxReading {
    fn max_reading(self) -> Self;
}

impl MaxReading for ReadStatus {
    /// Opening a book at 0% still counts as "reading".
    fn max_reading(self) -> Self {
        match self {
            ReadStatus::Unread => ReadStatus::Reading,
            s => s,
        }
    }
}

mod chrono_like {
    /// Days since 1970-01-01 for a `YYYY-MM-DD` string (proleptic Gregorian).
    pub fn days_from_civil(s: &str) -> Option<i64> {
        let mut it = s.split('-');
        let y: i64 = it.next()?.parse().ok()?;
        let m: i64 = it.next()?.parse().ok()?;
        let d: i64 = it.next()?.parse().ok()?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(era * 146097 + doe - 719468)
    }

    #[test]
    fn epoch() {
        assert_eq!(days_from_civil("1970-01-01"), Some(0));
        assert_eq!(
            days_from_civil("2024-03-01").unwrap() - days_from_civil("2024-02-28").unwrap(),
            2
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::lib_with_tree;
    use bc_core::FitMode;

    fn page(p: u32) -> Locator {
        Locator::Pdf {
            page: p,
            offset: 0.0,
            fit: FitMode::Width,
            zoom: 1.0,
        }
    }

    #[test]
    fn save_restore_and_continue_reading() {
        let (lib, _) = lib_with_tree();
        let id = lib.book_id_by_key("drive:found").unwrap().unwrap();
        assert!(lib.continue_reading(10).unwrap().is_empty());
        lib.save_progress(id, "me", &page(41), 0.2, Some("2026-10-08"))
            .unwrap();
        let s = lib.book_summary(id).unwrap();
        assert_eq!(s.status, ReadStatus::Reading);
        assert!((s.percent - 0.2).abs() < 1e-9);
        assert_eq!(lib.continue_reading(10).unwrap()[0].id, id);
        let (pos, prompt) = lib.resume_point(id, "me").unwrap();
        assert_eq!(pos.unwrap().locator, page(41));
        assert!(prompt.is_none());

        lib.save_progress(id, "me", &page(499), 1.0, None).unwrap();
        assert_eq!(lib.book_summary(id).unwrap().status, ReadStatus::Finished);
        assert!(lib.continue_reading(10).unwrap().is_empty());
    }

    #[test]
    fn other_device_prompt() {
        let (lib, _) = lib_with_tree();
        let id = lib.book_id_by_key("drive:found").unwrap().unwrap();
        lib.save_progress(id, "me", &page(10), 0.02, None).unwrap();
        let now = bc_core::now_ms();
        lib.upsert_remote_progress(
            id,
            &BookEntry {
                locator: page(211),
                percent: 0.42,
                status: ReadStatus::Reading,
                updated_at: now + 1000,
                device_id: "tablet".into(),
            },
        )
        .unwrap();
        let (pos, prompt) = lib.resume_point(id, "me").unwrap();
        assert_eq!(pos.unwrap().locator, page(10));
        assert_eq!(prompt.unwrap().label, "page 212");
        // Fresh device: just start at the other device's position.
        let (pos, prompt) = lib.resume_point(id, "new-laptop").unwrap();
        assert_eq!(pos.unwrap().locator, page(211));
        assert!(prompt.is_none());
    }

    #[test]
    fn manual_status_and_stats() {
        let (lib, _) = lib_with_tree();
        let id = lib.book_id_by_key("drive:dune").unwrap().unwrap();
        lib.set_status_override(id, Some(ReadStatus::Finished))
            .unwrap();
        let st = lib.stats("2026-10-08").unwrap();
        assert_eq!(st.finished, 1);
        assert_eq!(st.total_books, 4);
        assert_eq!(st.rank.name, "Bookworm");
    }

    #[test]
    fn streaks() {
        let (lib, _) = lib_with_tree();
        let id = lib.book_id_by_key("drive:dune").unwrap().unwrap();
        let loc = Locator::Epub {
            cfi: "x".into(),
            href: None,
            percent: 0.1,
        };
        for d in ["2026-10-05", "2026-10-06", "2026-10-07", "2026-10-03"] {
            lib.save_progress(id, "me", &loc, 0.1, Some(d)).unwrap();
        }
        assert_eq!(
            lib.stats("2026-10-08").unwrap().streak_days,
            3,
            "yesterday keeps the streak"
        );
        assert_eq!(lib.stats("2026-10-07").unwrap().streak_days, 3);
        assert_eq!(lib.stats("2026-10-10").unwrap().streak_days, 0);
    }
}
