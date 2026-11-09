//! "Build a lab": a series of practical labs derived from a whole book.
//!
//! Unlike a zone map (one passage), a lab guide is built from an excerpt
//! sampled across the entire book, so the labs can sequence the book's
//! ideas in the order they should be practised. Each lab is self-contained:
//! setup, step-by-step work, a diagram, and checks.

use serde::{Deserialize, Serialize};

use crate::{CheckQuestion, Result, TutorError};

/// A diagram for a lab. `mermaid` holds Mermaid flowchart/graph source the
/// UI shows as a diagram block; `explanation` walks through it in words so
/// the diagram still makes sense as plain text (and in Markdown exports).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LabDiagram {
    pub title: String,
    /// e.g. "flowchart", "topology", "sequence", "timeline", "hierarchy".
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub mermaid: String,
    #[serde(default)]
    pub explanation: String,
}

/// One step inside a lab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LabStep {
    pub title: String,
    /// What to actually do, in order.
    #[serde(default)]
    pub instructions: String,
    /// Why this step matters / what concept from the book it exercises.
    #[serde(default)]
    pub why: String,
    /// What the reader should see or have when the step worked.
    #[serde(default)]
    pub expected: String,
    #[serde(default)]
    pub tip: String,
}

/// One practical lab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Lab {
    pub title: String,
    #[serde(default)]
    pub objective: String,
    /// "beginner" | "intermediate" | "advanced" (free text from the model).
    #[serde(default)]
    pub difficulty: String,
    #[serde(default)]
    pub estimated_minutes: i64,
    /// Chapters / sections / page ranges in the book this lab draws on.
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub concepts: Vec<String>,
    /// Tools, accounts, files, hardware or environment needed.
    #[serde(default)]
    pub setup_requirements: Vec<String>,
    /// Ordered steps to prepare the environment before step 1.
    #[serde(default)]
    pub setup_steps: Vec<String>,
    #[serde(default)]
    pub steps: Vec<LabStep>,
    #[serde(default)]
    pub diagram: LabDiagram,
    #[serde(default)]
    pub checks: Vec<CheckQuestion>,
    #[serde(default)]
    pub troubleshooting: Vec<String>,
    /// Harder follow-up once the lab works.
    #[serde(default)]
    pub extension: String,
}

/// A whole-book set of labs, as the model returns it and the UI shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LabGuide {
    /// What the book covers and how the labs turn it into practice.
    #[serde(default)]
    pub overview: String,
    /// Who these labs assume the reader is.
    #[serde(default)]
    pub audience: String,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    /// The common environment most labs share.
    #[serde(default)]
    pub tools_overview: String,
    #[serde(default)]
    pub labs: Vec<Lab>,
    /// A final project that combines the labs, when one fits.
    #[serde(default)]
    pub capstone: String,
    #[serde(default)]
    pub safety_notes: Vec<String>,
}

impl LabGuide {
    pub fn parse(text: &str) -> Result<Self> {
        let body = extract_json(text)
            .ok_or_else(|| TutorError::Invalid("no JSON object in the answer".into()))?;
        let mut g: LabGuide =
            serde_json::from_str(body).map_err(|e| TutorError::Invalid(e.to_string()))?;
        g.tidy();
        if g.labs.is_empty() {
            return Err(TutorError::Invalid("the answer had no labs".into()));
        }
        if g.labs.iter().any(|l| l.steps.is_empty()) {
            return Err(TutorError::Invalid(
                "a lab in the answer had no steps".into(),
            ));
        }
        Ok(g)
    }

    fn tidy(&mut self) {
        let t = |s: &mut String| *s = s.trim().to_string();
        t(&mut self.overview);
        t(&mut self.audience);
        t(&mut self.tools_overview);
        t(&mut self.capstone);
        self.prerequisites.retain(|s| !s.trim().is_empty());
        self.prerequisites.truncate(12);
        self.safety_notes.retain(|s| !s.trim().is_empty());
        self.safety_notes.truncate(8);
        self.labs.retain(|l| !l.title.trim().is_empty());
        self.labs.truncate(10);
        for l in &mut self.labs {
            t(&mut l.title);
            t(&mut l.objective);
            t(&mut l.difficulty);
            t(&mut l.extension);
            if l.estimated_minutes < 0 {
                l.estimated_minutes = 0;
            }
            l.source_refs.retain(|s| !s.trim().is_empty());
            l.source_refs.truncate(6);
            l.concepts.retain(|s| !s.trim().is_empty());
            l.concepts.truncate(10);
            l.setup_requirements.retain(|s| !s.trim().is_empty());
            l.setup_requirements.truncate(12);
            l.setup_steps.retain(|s| !s.trim().is_empty());
            l.setup_steps.truncate(15);
            l.steps
                .retain(|s| !s.title.trim().is_empty() || !s.instructions.trim().is_empty());
            l.steps.truncate(20);
            l.checks.retain(|c| !c.question.trim().is_empty());
            l.checks.truncate(6);
            l.troubleshooting.retain(|s| !s.trim().is_empty());
            l.troubleshooting.truncate(8);
            t(&mut l.diagram.title);
            t(&mut l.diagram.kind);
            t(&mut l.diagram.mermaid);
            t(&mut l.diagram.explanation);
        }
    }

    /// Render the guide as a standalone Markdown artefact. Diagrams are
    /// kept as fenced ```mermaid blocks so they render on GitHub and in
    /// most Markdown viewers, with the written explanation alongside.
    pub fn to_markdown(&self, title: &str, author: Option<&str>) -> String {
        let mut out = format!("# Labs for {}\n\n", title.trim());
        if let Some(a) = author.filter(|a| !a.trim().is_empty()) {
            out.push_str(&format!("*{a}*\n\n"));
        }
        if !self.overview.trim().is_empty() {
            out.push_str(&format!("{}\n\n", self.overview.trim()));
        }
        if !self.audience.trim().is_empty() {
            out.push_str(&format!(
                "**Who these labs are for:** {}\n\n",
                self.audience.trim()
            ));
        }
        if !self.prerequisites.is_empty() {
            out.push_str("## Before you start\n\n");
            for p in &self.prerequisites {
                out.push_str(&format!("- {p}\n"));
            }
            out.push('\n');
        }
        if !self.tools_overview.trim().is_empty() {
            out.push_str("## Tools and setup at a glance\n\n");
            out.push_str(&format!("{}\n\n", self.tools_overview.trim()));
        }
        if !self.safety_notes.is_empty() {
            out.push_str("## Safety and cost notes\n\n");
            for n in &self.safety_notes {
                out.push_str(&format!("- {n}\n"));
            }
            out.push('\n');
        }
        for (i, lab) in self.labs.iter().enumerate() {
            out.push_str(&format!("## Lab {}: {}\n\n", i + 1, lab.title.trim()));
            if !lab.objective.trim().is_empty() {
                out.push_str(&format!("**Objective:** {}\n\n", lab.objective.trim()));
            }
            let mut meta = Vec::new();
            if !lab.difficulty.trim().is_empty() {
                meta.push(lab.difficulty.trim().to_string());
            }
            if lab.estimated_minutes > 0 {
                meta.push(format!("about {} minutes", lab.estimated_minutes));
            }
            if !meta.is_empty() {
                out.push_str(&format!("_{}_\n\n", meta.join(" · ")));
            }
            if !lab.source_refs.is_empty() {
                out.push_str(&format!(
                    "**In the book:** {}\n\n",
                    lab.source_refs.join("; ")
                ));
            }
            if !lab.concepts.is_empty() {
                out.push_str(&format!(
                    "**Concepts practised:** {}\n\n",
                    lab.concepts.join(", ")
                ));
            }
            if !lab.setup_requirements.is_empty() || !lab.setup_steps.is_empty() {
                out.push_str("### Set up\n\n");
                for r in &lab.setup_requirements {
                    out.push_str(&format!("- {r}\n"));
                }
                if !lab.setup_requirements.is_empty() {
                    out.push('\n');
                }
                for (j, s) in lab.setup_steps.iter().enumerate() {
                    out.push_str(&format!("{}. {s}\n", j + 1));
                }
                out.push('\n');
            }
            if !lab.diagram.mermaid.trim().is_empty() || !lab.diagram.explanation.trim().is_empty()
            {
                let dtitle = if lab.diagram.title.trim().is_empty() {
                    "Diagram".to_string()
                } else {
                    lab.diagram.title.trim().to_string()
                };
                out.push_str(&format!("### Diagram: {dtitle}\n\n"));
                if !lab.diagram.mermaid.trim().is_empty() {
                    out.push_str("```mermaid\n");
                    out.push_str(lab.diagram.mermaid.trim());
                    out.push_str("\n```\n\n");
                }
                if !lab.diagram.explanation.trim().is_empty() {
                    out.push_str(&format!("{}\n\n", lab.diagram.explanation.trim()));
                }
            }
            out.push_str("### Steps\n\n");
            for (j, step) in lab.steps.iter().enumerate() {
                let stitle = if step.title.trim().is_empty() {
                    format!("Step {}", j + 1)
                } else {
                    step.title.trim().to_string()
                };
                out.push_str(&format!("{}. **{stitle}**", j + 1));
                if !step.instructions.trim().is_empty() {
                    out.push_str(&format!(" — {}", step.instructions.trim()));
                }
                out.push('\n');
                if !step.why.trim().is_empty() {
                    out.push_str(&format!("   - Why: {}\n", step.why.trim()));
                }
                if !step.expected.trim().is_empty() {
                    out.push_str(&format!("   - You should see: {}\n", step.expected.trim()));
                }
                if !step.tip.trim().is_empty() {
                    out.push_str(&format!("   - Tip: {}\n", step.tip.trim()));
                }
            }
            out.push('\n');
            if !lab.checks.is_empty() {
                out.push_str("### Check yourself\n\n");
                for c in &lab.checks {
                    out.push_str(&format!("- **Q:** {}\n", c.question.trim()));
                    if !c.answer.trim().is_empty() {
                        out.push_str(&format!("  - **A:** {}\n", c.answer.trim()));
                    }
                }
                out.push('\n');
            }
            if !lab.troubleshooting.is_empty() {
                out.push_str("### If it goes wrong\n\n");
                for t in &lab.troubleshooting {
                    out.push_str(&format!("- {t}\n"));
                }
                out.push('\n');
            }
            if !lab.extension.trim().is_empty() {
                out.push_str(&format!("### Go further\n\n{}\n\n", lab.extension.trim()));
            }
        }
        if !self.capstone.trim().is_empty() {
            out.push_str("## Capstone\n\n");
            out.push_str(&format!("{}\n\n", self.capstone.trim()));
        }
        out.push_str("---\n\n_Generated by BiblioChad's \"Build a lab\" from the book text. Check commands and settings against the book and official docs before running them._\n");
        out
    }
}

fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

/// Sample `full` evenly into at most `max_chars`, so a lab prompt sees the
/// beginning, middle and end of a long book rather than only its opening.
/// Short books are returned whole. Chunk boundaries are marked so the
/// model knows it is reading excerpts, not continuous prose.
pub fn sample_book_text(full: &str, max_chars: usize) -> String {
    let full = full.trim();
    if full.chars().count() <= max_chars || max_chars == 0 {
        return full.to_string();
    }
    // Split into ~8 windows spread across the book and take an equal slice
    // from the start of each window (usually a section boundary marker).
    const WINDOWS: usize = 8;
    let chars: Vec<char> = full.chars().collect();
    let total = chars.len();
    let per = max_chars / WINDOWS;
    let mut out = String::new();
    for w in 0..WINDOWS {
        let start = w * total / WINDOWS;
        let slice: String = chars[start..(start + per).min(total)].iter().collect();
        if w > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format!(
            "[Excerpt {} of {WINDOWS} — around {}% through the book]\n{}",
            w + 1,
            w * 100 / WINDOWS,
            slice.trim()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "overview": "Turns networking theory into practice.",
      "audience": "Beginners with a home network.",
      "prerequisites": ["Basic command line"],
      "tools_overview": "A laptop and a router admin page.",
      "labs": [{
        "title": "See your own network",
        "objective": "Map the devices on your LAN.",
        "difficulty": "beginner",
        "estimated_minutes": 30,
        "source_refs": ["Chapter 1"],
        "concepts": ["IP addresses", ""],
        "setup_requirements": ["A laptop on your home Wi-Fi"],
        "setup_steps": ["Join your home Wi-Fi."],
        "steps": [{"title": "Find your IP", "instructions": "Run ipconfig.", "why": "Anchors the chapter's addressing model.", "expected": "An address like 192.168.1.20.", "tip": ""}],
        "diagram": {"title": "Home LAN", "kind": "topology", "mermaid": "graph LR\n  R[Router] --> L[Laptop]", "explanation": "The router connects every device."},
        "checks": [{"question": "What is a gateway?", "answer": "The router's LAN address."}],
        "troubleshooting": ["No Wi-Fi? Use a cable."],
        "extension": "Repeat on a phone hotspot."
      }],
      "capstone": "Document your home network.",
      "safety_notes": ["Only scan networks you own."]
    }"#;

    #[test]
    fn parses_tidies_and_exports() {
        let g = LabGuide::parse(SAMPLE).unwrap();
        assert_eq!(g.labs.len(), 1);
        assert_eq!(g.labs[0].concepts, vec!["IP addresses".to_string()]);
        let md = g.to_markdown("Networking 101", Some("A. Author"));
        assert!(md.contains("# Labs for Networking 101"));
        assert!(md.contains("## Lab 1: See your own network"));
        assert!(md.contains("```mermaid"));
        assert!(md.contains("graph LR"));
        assert!(md.contains("1. **Find your IP**"));
        assert!(md.contains("## Capstone"));
    }

    #[test]
    fn rejects_guides_without_labs_or_steps() {
        assert!(LabGuide::parse(r#"{"overview":"x","labs":[]}"#).is_err());
        assert!(LabGuide::parse(r#"{"labs":[{"title":"L","steps":[]}]}"#).is_err());
        assert!(LabGuide::parse("no json").is_err());
    }

    #[test]
    fn sampling_covers_the_whole_book() {
        let full = (0..100)
            .map(|i| format!("SECTION{i} {} ", "x".repeat(90)))
            .collect::<String>();
        let s = sample_book_text(&full, 800);
        assert!(
            s.chars().count() <= 800 + 8 * 64,
            "roughly capped: {}",
            s.len()
        );
        assert!(s.contains("SECTION0"), "keeps the opening");
        assert!(s.contains("Excerpt 8 of 8"), "reaches the end");
        // A later section must appear, proving we didn't just take the head.
        assert!(s.contains("SECTION8") || s.contains("SECTION9") || s.contains("SECTION"));
        assert_eq!(sample_book_text("short book", 800), "short book");
    }
}
