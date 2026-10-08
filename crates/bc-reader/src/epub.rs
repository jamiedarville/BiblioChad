//! EPUB container access. Every EPUB is treated as untrusted: entry names are
//! validated against zip-slip style paths, and reads are capped to defend
//! against decompression bombs.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::{BookMetadata, ReaderError, Result};

/// Maximum number of entries we accept in one archive.
pub const MAX_ENTRIES: usize = 50_000;
/// Maximum uncompressed size of a single entry we will read.
pub const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
/// Entries above this size must not exceed [`MAX_RATIO`] compression ratio.
const RATIO_CHECK_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RATIO: u64 = 200;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EntryInfo {
    pub name: String,
    pub size: u64,
}

/// Parsed OPF package summary.
#[derive(Debug, Clone, Default)]
pub struct Package {
    pub opf_path: String,
    pub metadata: BookMetadata,
    /// Archive path of the cover image, if one could be found.
    pub cover_path: Option<String>,
    /// Archive paths of the spine items, in reading order.
    pub spine: Vec<String>,
}

pub struct EpubArchive {
    zip: zip::ZipArchive<BufReader<File>>,
    index: HashMap<String, usize>,
    entries: Vec<EntryInfo>,
}

/// Returns true when an archive entry name is safe to expose.
pub fn is_safe_entry_name(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') || name.contains('\\') {
        return false;
    }
    if name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
        return false;
    }
    !name.split('/').any(|seg| seg == "..")
}

impl EpubArchive {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let zip = zip::ZipArchive::new(BufReader::new(file))
            .map_err(|e| ReaderError::BadEpub(e.to_string()))?;
        if zip.len() > MAX_ENTRIES {
            return Err(ReaderError::BadEpub(format!("{} entries", zip.len())));
        }
        let mut archive = Self {
            zip,
            index: HashMap::new(),
            entries: Vec::new(),
        };
        for i in 0..archive.zip.len() {
            let f = archive
                .zip
                .by_index_raw(i)
                .map_err(|e| ReaderError::BadEpub(e.to_string()))?;
            if f.is_dir() {
                continue;
            }
            let name = f.name().to_string();
            if !is_safe_entry_name(&name) {
                tracing::warn!(entry = %name, "skipping unsafe EPUB entry");
                continue;
            }
            archive.entries.push(EntryInfo {
                name: name.clone(),
                size: f.size(),
            });
            archive.index.insert(name, i);
        }
        Ok(archive)
    }

    pub fn entries(&self) -> &[EntryInfo] {
        &self.entries
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Read one entry fully, with size and ratio limits enforced on the
    /// actual decompressed stream (headers can lie).
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        if !is_safe_entry_name(name) {
            return Err(ReaderError::UnsafeEntry(name.into()));
        }
        let idx = *self
            .index
            .get(name)
            .ok_or_else(|| ReaderError::NotFound(name.into()))?;
        let mut f = self
            .zip
            .by_index(idx)
            .map_err(|e| ReaderError::BadEpub(e.to_string()))?;
        let compressed = f.compressed_size().max(1);
        let declared = f.size();
        if declared > MAX_ENTRY_BYTES {
            return Err(ReaderError::TooLarge(name.into()));
        }
        let limit = if declared > RATIO_CHECK_BYTES {
            declared.min(compressed.saturating_mul(MAX_RATIO))
        } else {
            MAX_ENTRY_BYTES.min(compressed.saturating_mul(MAX_RATIO).max(RATIO_CHECK_BYTES))
        };
        let mut buf = Vec::with_capacity(declared.min(limit) as usize);
        let read = (&mut f).take(limit + 1).read_to_end(&mut buf)? as u64;
        if read > limit {
            return Err(ReaderError::TooLarge(name.into()));
        }
        Ok(buf)
    }

    pub fn read_string(&mut self, name: &str) -> Result<String> {
        let bytes = self.read(name)?;
        Ok(String::from_utf8_lossy(strip_bom(&bytes)).into_owned())
    }

    /// Locate the OPF via META-INF/container.xml and parse it.
    pub fn package(&mut self) -> Result<Package> {
        let container = self
            .read_string("META-INF/container.xml")
            .map_err(|_| ReaderError::BadEpub("missing META-INF/container.xml".into()))?;
        let opf_path = find_rootfile(&container)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|e| e.name.ends_with(".opf"))
                    .map(|e| e.name.clone())
            })
            .ok_or_else(|| ReaderError::BadEpub("no OPF rootfile".into()))?;
        let opf = self.read_string(&opf_path)?;
        let mut pkg = parse_opf(&opf, &opf_path)?;
        if let Some(cover) = &pkg.cover_path {
            if !self.contains(cover) {
                pkg.cover_path = None;
            }
        }
        Ok(pkg)
    }
}

fn strip_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)
}

fn local_name(e: &BytesStart) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase()
}

fn attr(e: &BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        let k = String::from_utf8_lossy(a.key.local_name().as_ref()).to_ascii_lowercase();
        if k == key {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}

fn find_rootfile(container_xml: &str) -> Option<String> {
    let mut r = Reader::from_str(container_xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local_name(&e) == "rootfile" => {
                let mt = attr(&e, "media-type");
                if mt
                    .as_deref()
                    .is_none_or(|m| m == "application/oebps-package+xml")
                {
                    return attr(&e, "full-path").map(|p| decode_href(&p));
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn decode_href(href: &str) -> String {
    percent_encoding::percent_decode_str(href)
        .decode_utf8_lossy()
        .into_owned()
}

/// Resolve `href` (relative to the directory of `base`) to an archive path.
pub fn resolve_href(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let href = decode_href(href);
    let mut parts: Vec<&str> = match base.rfind('/') {
        Some(i) => base[..i].split('/').collect(),
        None => Vec::new(),
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[derive(Default)]
struct ManifestItem {
    href: String,
    media_type: String,
    properties: String,
}

fn looks_like_isbn(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s
        .strip_prefix("urn:isbn:")
        .or_else(|| s.strip_prefix("isbn:"))
        .or_else(|| s.strip_prefix("ISBN "))
        .unwrap_or(s);
    let digits: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X')
        .collect();
    let only_isbn_chars = s
        .chars()
        .all(|c| c.is_ascii_digit() || c == '-' || c == ' ' || c == 'X');
    if only_isbn_chars && (digits.len() == 10 || digits.len() == 13) {
        Some(digits)
    } else {
        None
    }
}

/// Parse an OPF document. `opf_path` is its path inside the archive and is
/// used to resolve manifest hrefs.
pub fn parse_opf(xml: &str, opf_path: &str) -> Result<Package> {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);

    let mut md = BookMetadata::default();
    let mut manifest: HashMap<String, ManifestItem> = HashMap::new();
    let mut spine_ids: Vec<String> = Vec::new();
    let mut cover_id_epub2: Option<String> = None;
    let mut guide_cover: Option<String> = None;
    let mut series_index_epub2: Option<f64> = None;
    // EPUB3 collection metadata: id -> name, refines id -> position.
    let mut collections: Vec<(Option<String>, String)> = Vec::new();
    let mut refines_position: HashMap<String, f64> = HashMap::new();
    let mut refines_type: HashMap<String, String> = HashMap::new();
    let mut identifiers: Vec<(Option<String>, String)> = Vec::new();

    // Current text-collecting element.
    let mut current: Option<(String, BytesStart<'static>)> = None;
    let mut text = String::new();
    let mut in_metadata = false;

    loop {
        let ev = r
            .read_event()
            .map_err(|e| ReaderError::BadEpub(format!("OPF: {e}")))?;
        match ev {
            Event::Start(e) => {
                let name = local_name(&e);
                if name == "metadata" {
                    in_metadata = true;
                } else if in_metadata && current.is_none() {
                    current = Some((name, e.into_owned()));
                    text.clear();
                }
            }
            Event::Empty(e) => {
                let name = local_name(&e);
                match name.as_str() {
                    "item" => {
                        if let (Some(id), Some(href)) = (attr(&e, "id"), attr(&e, "href")) {
                            manifest.insert(
                                id,
                                ManifestItem {
                                    href: resolve_href(opf_path, &href),
                                    media_type: attr(&e, "media-type").unwrap_or_default(),
                                    properties: attr(&e, "properties").unwrap_or_default(),
                                },
                            );
                        }
                    }
                    "itemref" => {
                        if let Some(id) = attr(&e, "idref") {
                            spine_ids.push(id);
                        }
                    }
                    "meta" if in_metadata => {
                        let n = attr(&e, "name").unwrap_or_default();
                        let c = attr(&e, "content").unwrap_or_default();
                        match n.as_str() {
                            "cover" => cover_id_epub2 = Some(c),
                            "calibre:series" if !c.trim().is_empty() => {
                                md.series = Some(c.trim().to_string())
                            }
                            "calibre:series_index" => series_index_epub2 = c.trim().parse().ok(),
                            _ => {}
                        }
                    }
                    "reference" if attr(&e, "type").as_deref() == Some("cover") => {
                        guide_cover = attr(&e, "href").map(|h| resolve_href(opf_path, &h));
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                if current.is_some() {
                    if let Ok(s) = t.unescape() {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        text.push_str(&s);
                    }
                }
            }
            Event::CData(t) => {
                if current.is_some() {
                    text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase();
                if name == "metadata" {
                    in_metadata = false;
                    current = None;
                    continue;
                }
                let Some((cur_name, start)) = current.as_ref() else {
                    continue;
                };
                if *cur_name != name {
                    continue;
                }
                let value = text.trim().to_string();
                if !value.is_empty() {
                    match cur_name.as_str() {
                        "title" if md.title.is_none() => md.title = Some(value),
                        "creator" => {
                            let role = attr(start, "role");
                            if role.as_deref().is_none_or(|r| r == "aut") {
                                md.authors.push(value);
                            }
                        }
                        "publisher" if md.publisher.is_none() => md.publisher = Some(value),
                        "language" if md.language.is_none() => md.language = Some(value),
                        "description" if md.description.is_none() => md.description = Some(value),
                        "identifier" => identifiers.push((attr(start, "scheme"), value)),
                        "meta" => {
                            let prop = attr(start, "property").unwrap_or_default();
                            let refines = attr(start, "refines")
                                .map(|r| r.trim_start_matches('#').to_string());
                            match (prop.as_str(), refines) {
                                ("belongs-to-collection", _) => {
                                    collections.push((attr(start, "id"), value))
                                }
                                ("group-position", Some(id)) => {
                                    if let Ok(p) = value.parse() {
                                        refines_position.insert(id, p);
                                    }
                                }
                                ("collection-type", Some(id)) => {
                                    refines_type.insert(id, value);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                current = None;
            }
            Event::Eof => break,
            _ => {}
        }
    }

    // Series: prefer EPUB3 collections typed "series" (or untyped), then calibre.
    if let Some((id, name)) = collections
        .iter()
        .find(|(id, _)| {
            id.as_ref()
                .and_then(|i| refines_type.get(i))
                .is_none_or(|t| t == "series")
        })
        .cloned()
    {
        md.series = Some(name);
        md.series_index = id.and_then(|i| refines_position.get(&i).copied());
    }
    if md.series_index.is_none() && md.series.is_some() {
        md.series_index = series_index_epub2;
    }

    md.isbn = identifiers
        .iter()
        .find(|(scheme, _)| {
            scheme
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("isbn"))
        })
        .and_then(|(_, v)| looks_like_isbn(v).or(Some(v.clone())))
        .or_else(|| identifiers.iter().find_map(|(_, v)| looks_like_isbn(v)));

    // Cover: EPUB3 property, then EPUB2 meta, then guide image, then heuristics.
    let is_image = |m: &ManifestItem| m.media_type.starts_with("image/");
    let cover_path = manifest
        .values()
        .find(|m| m.properties.split_whitespace().any(|p| p == "cover-image"))
        .map(|m| m.href.clone())
        .or_else(|| {
            cover_id_epub2.as_ref().and_then(|id| {
                manifest.get(id).map(|m| m.href.clone()).or_else(|| {
                    // Some books put the href in the cover meta instead of an id.
                    manifest
                        .values()
                        .find(|m| m.href.ends_with(id.as_str()))
                        .map(|m| m.href.clone())
                })
            })
        })
        .or_else(|| {
            guide_cover.as_ref().and_then(|g| {
                manifest
                    .values()
                    .find(|m| &m.href == g && is_image(m))
                    .map(|m| m.href.clone())
            })
        })
        .or_else(|| {
            let mut candidates: Vec<(&String, &ManifestItem)> = manifest
                .iter()
                .filter(|(id, m)| {
                    is_image(m)
                        && (id.to_ascii_lowercase().contains("cover")
                            || m.href.to_ascii_lowercase().contains("cover"))
                })
                .collect();
            candidates.sort_by(|a, b| a.1.href.cmp(&b.1.href));
            candidates.first().map(|(_, m)| m.href.clone())
        });

    let spine = spine_ids
        .iter()
        .filter_map(|id| manifest.get(id).map(|m| m.href.clone()))
        .collect();

    Ok(Package {
        opf_path: opf_path.to_string(),
        metadata: md,
        cover_path,
        spine,
    })
}

/// Best-effort mime type for an archive entry, by extension.
pub fn mime_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "xhtml" | "xht" => "application/xhtml+xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "opf" => "application/oebps-package+xml",
        "ncx" => "application/x-dtbncx+xml",
        "xml" => "application/xml",
        "smil" => "application/smil+xml",
        "mp3" => "audio/mpeg",
        "mp4" | "m4a" => "audio/mp4",
        "js" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub fn make_epub(path: &Path, opf: &str, extra: &[(&str, &[u8])]) {
        let f = File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        z.start_file("mimetype", stored).unwrap();
        z.write_all(b"application/epub+zip").unwrap();
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("META-INF/container.xml", opts).unwrap();
        z.write_all(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        )
        .unwrap();
        z.start_file("OEBPS/content.opf", opts).unwrap();
        z.write_all(opf.as_bytes()).unwrap();
        for (name, data) in extra {
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    pub const EPUB3_OPF: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:isbn:978-0-441-17271-9</dc:identifier>
    <dc:title>Dune</dc:title>
    <dc:creator id="c1">Frank Herbert</dc:creator>
    <dc:language>en</dc:language>
    <dc:publisher>Ace</dc:publisher>
    <dc:description><![CDATA[<p>Spice.</p>]]></dc:description>
    <meta property="belongs-to-collection" id="col">Dune Chronicles</meta>
    <meta refines="#col" property="collection-type">series</meta>
    <meta refines="#col" property="group-position">1</meta>
  </metadata>
  <manifest>
    <item id="c" href="images/cover%20art.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="ch1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/><itemref idref="ch2"/></spine>
</package>"##;

    #[test]
    fn parses_epub3() {
        let pkg = parse_opf(EPUB3_OPF, "OEBPS/content.opf").unwrap();
        assert_eq!(pkg.metadata.title.as_deref(), Some("Dune"));
        assert_eq!(pkg.metadata.authors, vec!["Frank Herbert"]);
        assert_eq!(pkg.metadata.series.as_deref(), Some("Dune Chronicles"));
        assert_eq!(pkg.metadata.series_index, Some(1.0));
        assert_eq!(pkg.metadata.isbn.as_deref(), Some("9780441172719"));
        assert_eq!(pkg.metadata.description.as_deref(), Some("<p>Spice.</p>"));
        assert_eq!(
            pkg.cover_path.as_deref(),
            Some("OEBPS/images/cover art.jpg")
        );
        assert_eq!(
            pkg.spine,
            vec!["OEBPS/text/ch1.xhtml", "OEBPS/text/ch2.xhtml"]
        );
    }

    #[test]
    fn parses_epub2_with_calibre_meta() {
        let opf = r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:opf="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Foundation</dc:title>
    <dc:creator opf:role="aut">Isaac Asimov</dc:creator>
    <dc:creator opf:role="ill">Someone Else</dc:creator>
    <dc:identifier opf:scheme="ISBN">0553293354</dc:identifier>
    <meta name="cover" content="cover-img"/>
    <meta name="calibre:series" content="Foundation"/>
    <meta name="calibre:series_index" content="1.0"/>
  </metadata>
  <manifest><item id="cover-img" href="../cover.png" media-type="image/png"/></manifest>
  <spine/>
</package>"#;
        let pkg = parse_opf(opf, "OPS/content.opf").unwrap();
        assert_eq!(pkg.metadata.authors, vec!["Isaac Asimov"]);
        assert_eq!(pkg.metadata.series.as_deref(), Some("Foundation"));
        assert_eq!(pkg.metadata.series_index, Some(1.0));
        assert_eq!(pkg.metadata.isbn.as_deref(), Some("0553293354"));
        assert_eq!(pkg.cover_path.as_deref(), Some("cover.png"));
    }

    #[test]
    fn heuristic_cover_and_missing_fields() {
        let opf = r#"<package><metadata><title>X</title></metadata>
  <manifest><item id="img1" href="Images/Cover.jpeg" media-type="image/jpeg"/></manifest></package>"#;
        let pkg = parse_opf(opf, "content.opf").unwrap();
        assert_eq!(pkg.metadata.title.as_deref(), Some("X"));
        assert!(pkg.metadata.authors.is_empty());
        assert_eq!(pkg.cover_path.as_deref(), Some("Images/Cover.jpeg"));
    }

    #[test]
    fn resolves_hrefs() {
        assert_eq!(resolve_href("OEBPS/content.opf", "../a/b.css"), "a/b.css");
        assert_eq!(resolve_href("content.opf", "./x.xhtml#frag"), "x.xhtml");
        assert_eq!(
            resolve_href("a/b/c.opf", "../../../../etc/passwd"),
            "etc/passwd"
        );
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(is_safe_entry_name("OEBPS/a.xhtml"));
        assert!(!is_safe_entry_name("../evil"));
        assert!(!is_safe_entry_name("a/../../evil"));
        assert!(!is_safe_entry_name("/etc/passwd"));
        assert!(!is_safe_entry_name("C:/x"));
        assert!(!is_safe_entry_name("a\\b"));
    }

    #[test]
    fn opens_archive_and_reads_entries() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b.epub");
        make_epub(
            &p,
            EPUB3_OPF,
            &[
                ("OEBPS/images/cover art.jpg", b"JPEGDATA"),
                ("../escape.txt", b"nope"),
            ],
        );
        let mut a = EpubArchive::open(&p).unwrap();
        assert!(a.contains("OEBPS/content.opf"));
        assert!(!a.entries().iter().any(|e| e.name.contains("..")));
        let pkg = a.package().unwrap();
        assert_eq!(
            pkg.cover_path.as_deref(),
            Some("OEBPS/images/cover art.jpg")
        );
        assert_eq!(a.read("OEBPS/images/cover art.jpg").unwrap(), b"JPEGDATA");
        assert!(matches!(
            a.read("../escape.txt"),
            Err(ReaderError::UnsafeEntry(_))
        ));
        assert!(matches!(a.read("nope"), Err(ReaderError::NotFound(_))));
    }

    #[test]
    fn rejects_decompression_bomb() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bomb.epub");
        let zeros = vec![0u8; 40 * 1024 * 1024];
        make_epub(&p, EPUB3_OPF, &[("OEBPS/bomb.bin", &zeros)]);
        let mut a = EpubArchive::open(&p).unwrap();
        assert!(matches!(
            a.read("OEBPS/bomb.bin"),
            Err(ReaderError::TooLarge(_))
        ));
    }

    #[test]
    fn missing_container_is_bad_epub() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.epub");
        let f = File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        z.start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"hi").unwrap();
        z.finish().unwrap();
        let mut a = EpubArchive::open(&p).unwrap();
        assert!(matches!(a.package(), Err(ReaderError::BadEpub(_))));
        let junk = dir.path().join("junk.epub");
        std::fs::write(&junk, b"not a zip").unwrap();
        assert!(EpubArchive::open(&junk).is_err());
    }
}
