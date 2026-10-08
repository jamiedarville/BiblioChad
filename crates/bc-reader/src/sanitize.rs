//! Strip active content from EPUB documents before they reach the webview.
//!
//! The reader iframe also runs under a strict CSP, but scripts are removed
//! here as well so that a CSP mistake is not enough to run book code.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};

fn is_dangerous_attr(key: &str, value: &str) -> bool {
    let k = key.to_ascii_lowercase();
    if k.starts_with("on") {
        return true;
    }
    let v: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        k.as_str(),
        "href" | "src" | "xlink:href" | "action" | "formaction" | "data"
    ) && (v.starts_with("javascript:") || v.starts_with("vbscript:"))
}

fn is_script_element(name: &[u8]) -> bool {
    let local = name.rsplit(|b| *b == b':').next().unwrap_or(name);
    local.eq_ignore_ascii_case(b"script")
}

fn is_blocked_element(name: &[u8]) -> bool {
    let local = name.rsplit(|b| *b == b':').next().unwrap_or(name);
    is_script_element(local)
        || local.eq_ignore_ascii_case(b"iframe")
        || local.eq_ignore_ascii_case(b"object")
        || local.eq_ignore_ascii_case(b"embed")
}

fn clean_start(e: &BytesStart) -> BytesStart<'static> {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut out = BytesStart::new(name);
    for a in e.attributes().with_checks(false).flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let value = a
            .unescape_value()
            .map(|v| v.into_owned())
            .unwrap_or_default();
        if is_dangerous_attr(&key, &value) {
            continue;
        }
        out.push_attribute((key.as_str(), value.as_str()));
    }
    out
}

/// Remove `<script>`, `<iframe>`, `<object>`, `<embed>`, `on*` handlers and
/// `javascript:` URLs from an XHTML/SVG document. Falls back to a byte-level
/// scrubber when the document is not well-formed XML.
pub fn sanitize_markup(input: &[u8]) -> Vec<u8> {
    match sanitize_xml(input) {
        Some(out) => out,
        None => sanitize_fallback(input),
    }
}

fn sanitize_xml(input: &[u8]) -> Option<Vec<u8>> {
    let mut reader = Reader::from_reader(input);
    reader.config_mut().check_end_names = false;
    let mut writer = Writer::new(Vec::with_capacity(input.len()));
    let mut skip_depth: usize = 0;
    let mut buf = Vec::new();
    loop {
        let ev = reader.read_event_into(&mut buf).ok()?;
        match ev {
            Event::Eof => break,
            Event::Start(e) => {
                if skip_depth > 0 || is_blocked_element(e.name().as_ref()) {
                    skip_depth += 1;
                } else {
                    writer.write_event(Event::Start(clean_start(&e))).ok()?;
                }
            }
            Event::End(e) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                } else {
                    writer.write_event(Event::End(e)).ok()?;
                }
            }
            Event::Empty(e) => {
                if skip_depth == 0 && !is_blocked_element(e.name().as_ref()) {
                    writer.write_event(Event::Empty(clean_start(&e))).ok()?;
                }
            }
            other => {
                if skip_depth == 0 {
                    writer.write_event(other).ok()?;
                }
            }
        }
        buf.clear();
    }
    Some(writer.into_inner())
}

/// Very conservative scrubber for tag soup: drops anything between
/// `<script` and `</script>` and neuters inline event handlers.
fn sanitize_fallback(input: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(input);
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if lower[i..].starts_with("<script") {
            match lower[i..].find("</script") {
                Some(end) => {
                    let close = lower[i + end..]
                        .find('>')
                        .map(|x| i + end + x + 1)
                        .unwrap_or(s.len());
                    i = close;
                }
                None => break,
            }
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    // Neutralise inline handlers like ` onload=` by renaming the attribute.
    let mut result = String::with_capacity(out.len());
    let bytes = out.as_bytes();
    let mut j = 0;
    while j < bytes.len() {
        if bytes[j].is_ascii_whitespace()
            && j + 3 < bytes.len()
            && bytes[j + 1].eq_ignore_ascii_case(&b'o')
            && bytes[j + 2].eq_ignore_ascii_case(&b'n')
            && bytes[j + 3].is_ascii_alphabetic()
        {
            result.push_str(" data-bc-blocked-");
            j += 1;
            continue;
        }
        let ch = out[j..].chars().next().unwrap();
        result.push(ch);
        j += ch.len_utf8();
    }
    result.replace("javascript:", "blocked:").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_handlers() {
        let doc = br#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><script type="text/javascript">alert(1)</script></head><body onload="evil()"><p>Hi <a href="javascript:evil()">x</a> <a href="ch2.xhtml">ok</a></p><iframe src="http://x"/><svg:script xmlns:svg="http://www.w3.org/2000/svg">bad()</svg:script></body></html>"#;
        let out = String::from_utf8(sanitize_markup(doc)).unwrap();
        assert!(!out.contains("alert"));
        assert!(!out.contains("onload"));
        assert!(!out.contains("javascript:"));
        assert!(!out.contains("iframe"));
        assert!(!out.contains("bad()"));
        assert!(out.contains("<p>Hi"));
        assert!(out.contains(r#"href="ch2.xhtml""#));
    }

    #[test]
    fn fallback_for_tag_soup() {
        let doc = b"<html><body onclick=x()><p>unclosed<br><script>evil()</script>after</body>";
        let out = String::from_utf8(sanitize_fallback(doc)).unwrap();
        assert!(!out.contains("evil"));
        assert!(!out.contains(" onclick"));
        assert!(out.contains("after"));
    }

    #[test]
    fn keeps_plain_content() {
        let doc = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>Caf\xc3\xa9 &amp; tea</p></body></html>";
        let out = String::from_utf8(sanitize_markup(doc)).unwrap();
        assert!(out.contains("Café &amp; tea"));
    }
}
