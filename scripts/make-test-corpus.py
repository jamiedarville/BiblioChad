#!/usr/bin/env python3
"""Generate a tiny test corpus (no downloads needed):

  chad-test.epub  EPUB 3 with cover, nav TOC, 6 chapters, and hostile markup
                  (<script>, onload) that the reader must neutralise.
  chad-test.pdf   40-page PDF with text on every page and an outline.

Usage: python3 scripts/make-test-corpus.py [out_dir]   (default: test-corpus/)
"""
import os
import struct
import sys
import zipfile
import zlib

out = sys.argv[1] if len(sys.argv) > 1 else "test-corpus"
os.makedirs(out, exist_ok=True)


def png(w, h, rgb):
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))

    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


LOREM = ("Chad opened the book and did not stop until the last page. "
         "Every chapter was a set, every paragraph a rep. ") * 12

chapters = []
for i in range(1, 7):
    body = "".join(f"<p>{LOREM}</p>" for _ in range(6))
    hostile = ('<script>document.title="pwned";alert(1)</script>' if i == 1 else "")
    chapters.append(f"""<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Chapter {i}</title>{hostile}<link rel="stylesheet" href="../style.css"/></head>
<body onload="alert('xss')"><h1 id="c{i}">Chapter {i}: Rep {i}</h1>{body}</body></html>""")

nav = """<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head><body><nav epub:type="toc"><h1>Contents</h1><ol>""" + "".join(
    f'<li><a href="text/ch{i}.xhtml">Chapter {i}: Rep {i}</a></li>' for i in range(1, 7)) + "</ol></nav></body></html>"

opf = """<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:isbn:9780000000002</dc:identifier>
    <dc:title>The Gigachad Reader</dc:title>
    <dc:creator>B. Chad</dc:creator>
    <dc:language>en</dc:language>
    <dc:publisher>Test Corpus Press</dc:publisher>
    <dc:description>A book written entirely for testing BiblioChad.</dc:description>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
    <meta property="belongs-to-collection" id="s">Chad Saga</meta>
    <meta refines="#s" property="group-position">1</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="cover" href="images/cover.png" media-type="image/png" properties="cover-image"/>
    <item id="css" href="style.css" media-type="text/css"/>
""" + "".join(f'    <item id="ch{i}" href="text/ch{i}.xhtml" media-type="application/xhtml+xml"/>\n' for i in range(1, 7)) + """  </manifest>
  <spine>""" + "".join(f'<itemref idref="ch{i}"/>' for i in range(1, 7)) + """</spine>
</package>"""

epub_path = os.path.join(out, "chad-test.epub")
with zipfile.ZipFile(epub_path, "w") as z:
    z.writestr(zipfile.ZipInfo("mimetype"), "application/epub+zip", compress_type=zipfile.ZIP_STORED)
    z.writestr("META-INF/container.xml", """<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>""",
               compress_type=zipfile.ZIP_DEFLATED)
    z.writestr("OEBPS/content.opf", opf, compress_type=zipfile.ZIP_DEFLATED)
    z.writestr("OEBPS/nav.xhtml", nav, compress_type=zipfile.ZIP_DEFLATED)
    z.writestr("OEBPS/style.css", "h1{color:#b5651d} p{margin:0 0 .8em}", compress_type=zipfile.ZIP_DEFLATED)
    z.writestr("OEBPS/images/cover.png", png(300, 450, (190, 90, 30)))
    for i, c in enumerate(chapters, 1):
        z.writestr(f"OEBPS/text/ch{i}.xhtml", c, compress_type=zipfile.ZIP_DEFLATED)

# --- PDF ---
pages = 40
objs = {}
font_id = 3
outline_id = 4
first_page_id = 10
page_ids = []
nid = first_page_id
for p in range(pages):
    page_ids.append((nid, nid + 1))
    nid += 2
bm_ids = list(range(nid, nid + 4))
objs[1] = f"<< /Type /Catalog /Pages 2 0 R /Outlines {outline_id} 0 R /PageMode /UseOutlines >>"
objs[2] = "<< /Type /Pages /Kids [" + " ".join(f"{a} 0 R" for a, _ in page_ids) + f"] /Count {pages} >>"
objs[font_id] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
info_id = 5
objs[info_id] = "<< /Title (Chad's Big PDF) /Author (Jamie) /Subject (A test PDF) >>"
objs[outline_id] = f"<< /Type /Outlines /First {bm_ids[0]} 0 R /Last {bm_ids[-1]} 0 R /Count {len(bm_ids)} >>"
for p, (pid, cid) in enumerate(page_ids):
    lines = [f"BT /F1 28 Tf 60 720 Td (Page {p + 1}) Tj ET"]
    for k in range(14):
        lines.append(f"BT /F1 12 Tf 60 {680 - k * 40} Td (Line {k + 1} of page {p + 1}: finish the book, then lift.) Tj ET")
    stream = "\n".join(lines).encode()
    objs[cid] = f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream"
    objs[pid] = (f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {cid} 0 R "
                 f"/Resources << /Font << /F1 {font_id} 0 R >> >> >>")
for k, bid in enumerate(bm_ids):
    target = page_ids[k * 10][0]
    prev = f"/Prev {bm_ids[k - 1]} 0 R " if k > 0 else ""
    nxt = f"/Next {bm_ids[k + 1]} 0 R " if k < len(bm_ids) - 1 else ""
    objs[bid] = f"<< /Title (Part {k + 1}) /Parent {outline_id} 0 R {prev}{nxt}/Dest [{target} 0 R /Fit] >>"

buf = b"%PDF-1.4\n"
offsets = {}
for oid in sorted(objs):
    offsets[oid] = len(buf)
    body = objs[oid] if isinstance(objs[oid], bytes) else objs[oid].encode()
    buf += f"{oid} 0 obj\n".encode() + body + b"\nendobj\n"
size = max(objs) + 1
xref = len(buf)
buf += f"xref\n0 {size}\n0000000000 65535 f \n".encode()
for oid in range(1, size):
    buf += (f"{offsets[oid]:010d} 00000 n \n" if oid in offsets else "0000000000 65535 f \n").encode()
buf += (f"trailer\n<< /Size {size} /Root 1 0 R /Info {info_id} 0 R >>\n"
        f"startxref\n{xref}\n%%EOF\n").encode()
with open(os.path.join(out, "chad-test.pdf"), "wb") as f:
    f.write(buf)
print(f"wrote {epub_path} and {os.path.join(out, 'chad-test.pdf')}")
