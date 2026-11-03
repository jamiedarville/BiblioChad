//! The shape of a zone map, as the model returns it and the UI shows it.

use serde::{Deserialize, Serialize};

use crate::{Result, TutorError};

/// Something the reader can already do or understand without help.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnownIdea {
    pub idea: String,
    /// Why we think they have it (their background, an earlier chapter...).
    #[serde(default)]
    pub because: String,
}

/// An idea inside the zone: reachable with support.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneItem {
    pub concept: String,
    /// Why it matters for this passage and the reader's goal.
    #[serde(default)]
    pub why_it_matters: String,
    /// Graduated scaffolding, lightest first. The UI reveals one at a time.
    #[serde(default)]
    pub hints: Vec<String>,
    /// The full explanation, for when the hints aren't enough.
    #[serde(default)]
    pub explanation: String,
    /// A small task or question to try with the hints.
    #[serde(default)]
    pub try_it: String,
    #[serde(default)]
    pub answer: String,
}

/// Something beyond the zone for now.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LaterIdea {
    pub idea: String,
    /// What to learn first, or when to come back to it.
    #[serde(default)]
    pub come_back_when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckQuestion {
    pub question: String,
    #[serde(default)]
    pub answer: String,
}

/// One map of the reader's Zone of Proximal Development for a passage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneGuide {
    /// One or two sentences on what the passage is doing.
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub can_do_alone: Vec<KnownIdea>,
    #[serde(default)]
    pub zone: Vec<ZoneItem>,
    #[serde(default)]
    pub not_yet: Vec<LaterIdea>,
    #[serde(default)]
    pub check: Vec<CheckQuestion>,
    /// A suggestion for what to do next while reading.
    #[serde(default)]
    pub next_step: String,
}

impl ZoneGuide {
    /// Parse model output. Structured outputs give clean JSON, but
    /// OpenAI-compatible servers vary, so code fences and leading prose are
    /// tolerated.
    pub fn parse(text: &str) -> Result<Self> {
        let body = extract_json(text)
            .ok_or_else(|| TutorError::Invalid("no JSON object in the answer".into()))?;
        let mut g: ZoneGuide =
            serde_json::from_str(body).map_err(|e| TutorError::Invalid(e.to_string()))?;
        g.tidy();
        if g.zone.is_empty() {
            return Err(TutorError::Invalid(
                "the answer had no ideas in the zone".into(),
            ));
        }
        Ok(g)
    }

    /// Drop empty entries and keep lists to a size that fits the panel.
    fn tidy(&mut self) {
        let t = |s: &mut String| *s = s.trim().to_string();
        t(&mut self.summary);
        t(&mut self.next_step);
        self.can_do_alone.retain(|k| !k.idea.trim().is_empty());
        self.can_do_alone.truncate(6);
        self.zone.retain(|z| !z.concept.trim().is_empty());
        self.zone.truncate(6);
        for z in &mut self.zone {
            z.hints.retain(|h| !h.trim().is_empty());
            z.hints.truncate(4);
        }
        self.not_yet.retain(|n| !n.idea.trim().is_empty());
        self.not_yet.truncate(5);
        self.check.retain(|c| !c.question.trim().is_empty());
        self.check.truncate(5);
    }

    pub fn concepts(&self) -> Vec<String> {
        self.zone.iter().map(|z| z.concept.clone()).collect()
    }
}

fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "summary": "Introduces opportunity cost.",
      "can_do_alone": [{"idea": "Trade-offs", "because": "everyday choices"}, {"idea": " ", "because": ""}],
      "zone": [{"concept": "Opportunity cost", "why_it_matters": "core idea",
                "hints": ["Think of what you give up.", "", "Value of the next best option."],
                "explanation": "The value of the best alternative forgone.",
                "try_it": "What is the cost of an hour of reading?", "answer": "The next best use of the hour."}],
      "not_yet": [{"idea": "Marginal analysis", "come_back_when": "after chapter 4"}],
      "check": [{"question": "Define opportunity cost.", "answer": "Best alternative forgone."}],
      "next_step": "Read on and look for examples."
    }"#;

    #[test]
    fn parses_and_tidies() {
        let g = ZoneGuide::parse(SAMPLE).unwrap();
        assert_eq!(g.can_do_alone.len(), 1);
        assert_eq!(g.zone[0].hints.len(), 2);
        assert_eq!(g.concepts(), vec!["Opportunity cost".to_string()]);
    }

    #[test]
    fn tolerates_fences_and_prose() {
        let wrapped = format!("Here you go:\n```json\n{SAMPLE}\n```");
        assert!(ZoneGuide::parse(&wrapped).is_ok());
    }

    #[test]
    fn rejects_empty_zone_and_garbage() {
        assert!(ZoneGuide::parse(r#"{"summary": "x", "zone": []}"#).is_err());
        assert!(ZoneGuide::parse("no json here").is_err());
    }
}
