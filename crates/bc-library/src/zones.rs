//! Learning-assistant data: per-book learner profiles and saved zone maps.
//! These stay on this device (they are not part of the Drive sync file).

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use bc_core::Locator;

use crate::{Library, LibraryError, Result};

pub const MIN_STRETCH: i64 = 1;
pub const MAX_STRETCH: i64 = 5;
pub const DEFAULT_STRETCH: i64 = 3;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ZoneProfile {
    pub book_id: i64,
    pub background: String,
    pub goal: String,
    pub stretch: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneFeedback {
    TooEasy,
    JustRight,
    TooHard,
}

impl ZoneFeedback {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TooEasy => "too_easy",
            Self::JustRight => "just_right",
            Self::TooHard => "too_hard",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "too_easy" => Some(Self::TooEasy),
            "just_right" => Some(Self::JustRight),
            "too_hard" => Some(Self::TooHard),
            _ => None,
        }
    }

    /// How the rating moves the stretch level.
    fn delta(self) -> i64 {
        match self {
            Self::TooEasy => 1,
            Self::JustRight => 0,
            Self::TooHard => -1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneMap {
    pub id: String,
    pub book_id: i64,
    pub locator: Locator,
    pub label: String,
    /// The guide as the tutor returned it (see `bc_tutor::ZoneGuide`).
    pub guide: serde_json::Value,
    pub model: String,
    pub feedback: Option<ZoneFeedback>,
    pub created_at: i64,
}

fn map_from_row(r: &Row) -> rusqlite::Result<Option<ZoneMap>> {
    let loc: String = r.get("locator_json")?;
    let guide: String = r.get("guide_json")?;
    let feedback: Option<String> = r.get("feedback")?;
    let (Ok(locator), Ok(guide)) = (Locator::from_json(&loc), serde_json::from_str(&guide)) else {
        return Ok(None);
    };
    Ok(Some(ZoneMap {
        id: r.get("id")?,
        book_id: r.get("book_id")?,
        locator,
        label: r.get("label")?,
        guide,
        model: r.get("model")?,
        feedback: feedback.as_deref().and_then(ZoneFeedback::parse),
        created_at: r.get("created_at")?,
    }))
}

impl Library {
    /// The reader's profile for a book (defaults if they haven't set one).
    pub fn zone_profile(&self, book_id: i64) -> Result<ZoneProfile> {
        let found = self
            .conn()
            .query_row(
                "SELECT background, goal, stretch FROM zone_profiles WHERE book_id = ?1",
                [book_id],
                |r| {
                    Ok(ZoneProfile {
                        book_id,
                        background: r.get(0)?,
                        goal: r.get(1)?,
                        stretch: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(found.unwrap_or(ZoneProfile {
            book_id,
            background: String::new(),
            goal: String::new(),
            stretch: DEFAULT_STRETCH,
        }))
    }

    /// Save what the reader told us. `stretch` is kept unless given.
    pub fn set_zone_profile(
        &self,
        book_id: i64,
        background: &str,
        goal: &str,
        stretch: Option<i64>,
    ) -> Result<ZoneProfile> {
        let current = self.zone_profile(book_id)?;
        let stretch = stretch
            .unwrap_or(current.stretch)
            .clamp(MIN_STRETCH, MAX_STRETCH);
        self.conn().execute(
            "INSERT INTO zone_profiles(book_id, background, goal, stretch, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(book_id) DO UPDATE SET background = excluded.background,
               goal = excluded.goal, stretch = excluded.stretch, updated_at = excluded.updated_at",
            params![
                book_id,
                background.trim(),
                goal.trim(),
                stretch,
                bc_core::now_ms()
            ],
        )?;
        self.zone_profile(book_id)
    }

    pub fn add_zone_map(
        &self,
        book_id: i64,
        locator: &Locator,
        label: &str,
        guide: &serde_json::Value,
        model: &str,
    ) -> Result<ZoneMap> {
        let id = bc_core::new_id();
        let now = bc_core::now_ms();
        self.conn().execute(
            "INSERT INTO zone_maps(id, book_id, locator_json, label, guide_json, model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                book_id,
                locator.to_json(),
                label,
                guide.to_string(),
                model,
                now
            ],
        )?;
        Ok(ZoneMap {
            id,
            book_id,
            locator: locator.clone(),
            label: label.to_string(),
            guide: guide.clone(),
            model: model.to_string(),
            feedback: None,
            created_at: now,
        })
    }

    /// Newest first.
    pub fn zone_maps(&self, book_id: i64, limit: u32) -> Result<Vec<ZoneMap>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT * FROM zone_maps WHERE book_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![book_id, limit], map_from_row)?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Rate a map. The book's stretch level moves with the rating; changing
    /// a rating undoes the previous one first, so re-rating can't run away.
    pub fn set_zone_feedback(&self, map_id: &str, feedback: ZoneFeedback) -> Result<ZoneProfile> {
        let (book_id, previous): (i64, Option<String>) = self
            .conn()
            .query_row(
                "SELECT book_id, feedback FROM zone_maps WHERE id = ?1",
                [map_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| LibraryError::NotFound(format!("zone map {map_id}")))?;
        let undo = previous
            .as_deref()
            .and_then(ZoneFeedback::parse)
            .map_or(0, ZoneFeedback::delta);
        let profile = self.zone_profile(book_id)?;
        let stretch = (profile.stretch - undo + feedback.delta()).clamp(MIN_STRETCH, MAX_STRETCH);
        self.conn().execute(
            "UPDATE zone_maps SET feedback = ?2 WHERE id = ?1",
            params![map_id, feedback.as_str()],
        )?;
        self.set_zone_profile(book_id, &profile.background, &profile.goal, Some(stretch))
    }

    pub fn delete_zone_map(&self, map_id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM zone_maps WHERE id = ?1", [map_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::lib_with_tree;
    use crate::BookQuery;

    #[test]
    fn profiles_maps_and_feedback() {
        let (lib, _) = lib_with_tree();
        let book = lib.query_books(&BookQuery::default()).unwrap()[0].id;

        let p = lib.zone_profile(book).unwrap();
        assert_eq!(p.stretch, DEFAULT_STRETCH);
        let p = lib
            .set_zone_profile(book, " Beginner ", "Pass the exam", None)
            .unwrap();
        assert_eq!(
            (p.background.as_str(), p.goal.as_str()),
            ("Beginner", "Pass the exam")
        );

        let loc = Locator::Pdf {
            page: 4,
            offset: 0.0,
            fit: Default::default(),
            zoom: 1.0,
        };
        let guide = serde_json::json!({ "zone": [{ "concept": "X" }] });
        let a = lib.add_zone_map(book, &loc, "page 5", &guide, "m").unwrap();
        let b = lib.add_zone_map(book, &loc, "page 6", &guide, "m").unwrap();
        let maps = lib.zone_maps(book, 10).unwrap();
        assert_eq!(maps.len(), 2);
        assert_eq!(maps[0].id, b.id, "newest first");
        assert_eq!(maps[1].guide, guide);

        // Too easy raises the stretch; changing to too hard undoes it first.
        assert_eq!(
            lib.set_zone_feedback(&a.id, ZoneFeedback::TooEasy)
                .unwrap()
                .stretch,
            4
        );
        assert_eq!(
            lib.set_zone_feedback(&a.id, ZoneFeedback::TooEasy)
                .unwrap()
                .stretch,
            4
        );
        assert_eq!(
            lib.set_zone_feedback(&a.id, ZoneFeedback::TooHard)
                .unwrap()
                .stretch,
            2
        );
        assert_eq!(
            lib.set_zone_feedback(&b.id, ZoneFeedback::TooHard)
                .unwrap()
                .stretch,
            1
        );
        assert_eq!(
            lib.set_zone_feedback(&b.id, ZoneFeedback::TooHard)
                .unwrap()
                .stretch,
            1
        );
        let p = lib.zone_profile(book).unwrap();
        assert_eq!(p.goal, "Pass the exam", "feedback keeps the profile text");

        lib.delete_zone_map(&a.id).unwrap();
        assert_eq!(lib.zone_maps(book, 10).unwrap().len(), 1);
        assert!(lib
            .set_zone_feedback("missing", ZoneFeedback::JustRight)
            .is_err());
    }
}
