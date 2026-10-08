use serde::{Deserialize, Serialize};

/// How a PDF page is fitted into the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    #[default]
    Width,
    Page,
    Custom,
}

/// A reading position inside a book.
///
/// EPUB: a CFI plus the chapter href and overall fraction.
/// PDF: zero-based page index, vertical offset within the page (0..1) and
/// the zoom / fit mode in effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Locator {
    Epub {
        cfi: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        href: Option<String>,
        #[serde(default)]
        percent: f64,
    },
    Pdf {
        page: u32,
        #[serde(default)]
        offset: f64,
        #[serde(default)]
        fit: FitMode,
        #[serde(default = "one")]
        zoom: f64,
    },
}

fn one() -> f64 {
    1.0
}

impl Locator {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("locator serializes")
    }

    pub fn from_json(s: &str) -> crate::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Short human label, e.g. "page 212" or "41%".
    pub fn label(&self, percent: f64) -> String {
        match self {
            Locator::Pdf { page, .. } => format!("page {}", page + 1),
            Locator::Epub { .. } => format!("{}%", (percent * 100.0).round() as i64),
        }
    }

    /// Whether two locators are far enough apart that a "continue from your
    /// other device?" prompt is worth showing.
    pub fn meaningfully_different(&self, other: &Locator, self_pct: f64, other_pct: f64) -> bool {
        match (self, other) {
            (Locator::Pdf { page: a, .. }, Locator::Pdf { page: b, .. }) => a.abs_diff(*b) >= 2,
            (Locator::Epub { cfi: a, .. }, Locator::Epub { cfi: b, .. }) => {
                a != b && (self_pct - other_pct).abs() >= 0.01
            }
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_epub() {
        let l = Locator::Epub {
            cfi: "epubcfi(/6/4!/4/2/1:0)".into(),
            href: Some("ch01.xhtml".into()),
            percent: 0.12,
        };
        let s = l.to_json();
        assert!(s.contains("\"kind\":\"epub\""));
        assert_eq!(Locator::from_json(&s).unwrap(), l);
    }

    #[test]
    fn pdf_defaults() {
        let l = Locator::from_json(r#"{"kind":"pdf","page":211}"#).unwrap();
        assert_eq!(
            l,
            Locator::Pdf { page: 211, offset: 0.0, fit: FitMode::Width, zoom: 1.0 }
        );
        assert_eq!(l.label(0.5), "page 212");
    }

    #[test]
    fn rejects_garbage() {
        assert!(Locator::from_json(r#"{"kind":"mobi"}"#).is_err());
    }

    #[test]
    fn meaningful_difference() {
        let a = Locator::Pdf { page: 10, offset: 0.0, fit: FitMode::Width, zoom: 1.0 };
        let b = Locator::Pdf { page: 11, offset: 0.5, fit: FitMode::Width, zoom: 1.0 };
        let c = Locator::Pdf { page: 40, offset: 0.0, fit: FitMode::Width, zoom: 1.0 };
        assert!(!a.meaningfully_different(&b, 0.1, 0.11));
        assert!(a.meaningfully_different(&c, 0.1, 0.4));
    }
}
