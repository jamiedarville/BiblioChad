#!/usr/bin/env python3
"""End-to-end smoke test through tauri-driver (WebDriver).

Launches the real app with the test corpus on the command line, so the EPUB
is imported and opened, then checks the reader rendered it, the hostile
script did not run, and the library lists both books. Screenshots go to
$E2E_OUT (default: e2e-out/).

Prereqs (Linux CI): xvfb, webkit2gtk-driver, `cargo install tauri-driver`,
a debug build with `--features tauri/custom-protocol`, and pdfium on
PDFIUM_DIR. See .github/workflows/ci.yml.
"""
import base64
import json
import os
import sys
import time
import urllib.request

DRIVER = os.environ.get("WEBDRIVER_URL", "http://127.0.0.1:4444")
APP = os.environ["BIBLIOCHAD_APP"]
CORPUS = os.environ.get("CORPUS", "test-corpus")
OUT = os.environ.get("E2E_OUT", "e2e-out")
os.makedirs(OUT, exist_ok=True)


def call(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(DRIVER + path, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read() or b"{}").get("value")


def wait(fn, timeout=30, what="condition"):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        try:
            last = fn()
            if last:
                return last
        except Exception as e:  # noqa: BLE001
            last = e
        time.sleep(0.5)
    raise AssertionError(f"timed out waiting for {what} (last: {last!r})")


session = call("POST", "/session", {"capabilities": {"alwaysMatch": {"tauri:options": {
    "application": APP,
    "args": [os.path.abspath(f"{CORPUS}/chad-test.epub"), os.path.abspath(f"{CORPUS}/chad-test.pdf")],
}}}})["sessionId"]
S = f"/session/{session}"


def js(script, *args):
    return call("POST", f"{S}/execute/sync", {"script": script, "args": list(args)})


def shot(name):
    png = base64.b64decode(call("GET", f"{S}/screenshot"))
    with open(f"{OUT}/{name}.png", "wb") as f:
        f.write(png)
    print(f"  screenshot {OUT}/{name}.png")


failures = []


def check(cond, msg):
    print(("  ok   " if cond else "  FAIL ") + msg)
    if not cond:
        failures.append(msg)


try:
    print("reader opens the EPUB passed on the command line")
    wait(lambda: js("return !!document.querySelector('foliate-view')"), what="foliate-view")
    # The footer shows the TOC label once foliate relocates.
    label = wait(lambda: js("return document.querySelector('.loc')?.textContent || ''"), what="location label")
    check("Chapter 1" in label, f"location label shows chapter ({label!r})")
    time.sleep(1.5)
    shot("01-epub-reader")
    check(js("return document.title") != "pwned", "book <script> did not run")

    print("page turn updates progress")
    before = js("return document.querySelector('.pct').textContent")
    for _ in range(6):
        js("window.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight'}))")
        time.sleep(0.4)
    after = js("return document.querySelector('.pct').textContent")
    check(before != after or after != "0%", f"percent moved ({before} -> {after})")

    print("TOC panel lists chapters")
    js("document.querySelector('button[aria-label=\"Table of contents\"]').click()")
    items = wait(lambda: js("return [...document.querySelectorAll('.toc button')].map(b => b.textContent)"), what="toc")
    check(len(items) == 6, f"toc has 6 entries ({len(items)})")
    js("document.querySelectorAll('.toc button')[3].click()")
    time.sleep(1.5)
    label = js("return document.querySelector('.loc').textContent")
    check("Chapter 4" in label, f"TOC navigation reached chapter 4 ({label!r})")
    shot("02-epub-toc")

    print("back to library; position is saved")
    time.sleep(2.5)  # debounce
    js("document.querySelector('button[aria-label=\"Back to library\"]').click()")
    titles = wait(lambda: js("return [...document.querySelectorAll('.cr .t')].map(e => e.textContent)"), what="continue reading")
    check("The Gigachad Reader" in titles, f"continue-reading shows the EPUB ({titles})")
    time.sleep(1)
    shot("03-library-shelves")

    print("all-books view lists both books with metadata")
    js("[...document.querySelectorAll('.tab')].find(b => b.textContent === 'All books').click()")
    names = wait(lambda: js("return [...document.querySelectorAll('.item .title')].map(e => e.textContent)"), what="grid")
    check("The Gigachad Reader" in names and "Chad's Big PDF" in names, f"grid lists both ({names})")
    shot("04-library-all")

    print("right-click menu survives releasing the secondary button (Linux WebKit)")
    js("""window.menuEvents = [];
          for (const type of ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'contextmenu', 'click', 'auxclick', 'resize', 'toggle']) {
            window.addEventListener(type, e => window.menuEvents.push({type, button: e.button, target: e.target?.className, menu: !!document.querySelector('.book-menu')}), true);
          }""")
    book = js("return document.querySelector('.item')")
    call("POST", f"{S}/actions", {"actions": [{
        "type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"},
        "actions": [
            {"type": "pointerMove", "duration": 0, "origin": book, "x": 0, "y": 0},
            {"type": "pointerDown", "button": 2},
            {"type": "pause", "duration": 250},
            {"type": "pointerUp", "button": 2},
        ],
    }]})
    try:
        wait(lambda: js("return document.querySelector('.book-menu')?.matches(':popover-open')"), what="context menu after right-button release")
    except AssertionError:
        print("context-menu diagnostics:", js("return {events: window.menuEvents, text: document.body.innerText, menu: document.querySelector('.book-menu')?.outerHTML}"), flush=True)
        shot("04-failed-context-menu")
        raise
    time.sleep(0.5)
    check(js("return document.querySelector('.book-menu')?.matches(':popover-open')"), "menu stays open after releasing right-click")
    js("document.querySelector('.book-menu button').click()")
    wait(lambda: js("return !!document.querySelector('#new-book-collection')"), what="collection picker")
    js("""const input = document.querySelector('#new-book-collection');
          input.value = 'Smoke test collection';
          input.dispatchEvent(new Event('input', {bubbles: true}));""")
    js("document.querySelector('.book-menu form').requestSubmit()")
    wait(lambda: js("return !document.querySelector('.book-menu')"), what="collection saved")
    # Reopen with the keyboard and verify the operation reached the backend.
    js("""document.querySelector('.item').focus();
          document.querySelector('.item').dispatchEvent(new KeyboardEvent('keydown', {key: 'F10', shiftKey: true, bubbles: true}));""")
    wait(lambda: js("return document.querySelector('.book-menu')?.matches(':popover-open')"), what="keyboard context menu")
    js("document.querySelector('.book-menu button').click()")
    wait(lambda: js("return [...document.querySelectorAll('.choices button')].some(b => b.textContent.includes('Smoke test collection') && b.disabled)"), what="saved collection membership")
    check(True, "collection saved and marked already added")
    shot("04a-collection-picker")
    js("window.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape'}))")
    wait(lambda: js("return !document.querySelector('.book-menu')"), what="Escape dismissal")
    js("document.querySelector('.item').dispatchEvent(new KeyboardEvent('keydown', {key: 'ContextMenu', bubbles: true}))")
    wait(lambda: js("return document.querySelector('.book-menu')?.matches(':popover-open')"), what="menu before outside click")
    # An outside primary-button press still dismisses the manual popover.
    call("POST", f"{S}/actions", {"actions": [{
        "type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"},
        "actions": [
            {"type": "pointerMove", "duration": 0, "origin": "viewport", "x": 2, "y": 2},
            {"type": "pointerDown", "button": 0},
            {"type": "pointerUp", "button": 0},
        ],
    }]})
    wait(lambda: js("return !document.querySelector('.book-menu')"), what="outside-click dismissal")
    check(True, "keyboard opening, Escape and outside-click dismissal work")

    print("PDF renders through PDFium")
    js("[...document.querySelectorAll('.item')].find(e => e.textContent.includes(\"Chad's Big PDF\")).click()")
    wait(lambda: js("return [...document.querySelectorAll('.page img')].some(i => i.complete && i.naturalWidth > 0)"), what="pdf page image")
    time.sleep(1)
    shot("05-pdf-reader")
    label = js("return document.querySelector('.loc').textContent")
    check(label.startswith("Page 1 of 40"), f"pdf location label ({label!r})")
    js("document.querySelector('.scroller').scrollTop = document.querySelector('.scroller').scrollHeight * 0.5")
    time.sleep(1)
    label = js("return document.querySelector('.loc').textContent")
    check("of 40" in label and not label.startswith("Page 1 "), f"scrolling moves pages ({label!r})")
    runs = wait(lambda: js("return document.querySelectorAll('.text-layer span').length"), what="text layer")
    check(runs > 0, f"text layer present ({runs} runs)")

    print("PDF search")
    js("document.querySelector('button[aria-label=\"Search in book\"]').click()")
    js("""const i = document.querySelector('input[aria-label="Find in book"]');
          i.value = 'page 37'; i.dispatchEvent(new Event('input', {bubbles: true}));
          i.form.requestSubmit();""")
    hits = wait(lambda: js("return document.querySelectorAll('.hits li').length"), what="search hits")
    check(hits >= 1, f"search found hits ({hits})")
    shot("06-pdf-search")

    print("settings page renders")
    js("document.querySelector('button[aria-label=\"Back to library\"]').click()")
    wait(lambda: js("return !!document.querySelector('button[aria-label=\"Settings\"]')"), what="library")
    js("document.querySelector('button[aria-label=\"Settings\"]').click()")
    wait(lambda: js("return document.querySelector('h1')?.textContent === 'Settings'"), what="settings")
    time.sleep(0.5)
    shot("07-settings")
finally:
    try:
        call("DELETE", S)
    except Exception:  # noqa: BLE001
        pass

if failures:
    print(f"\n{len(failures)} check(s) failed")
    sys.exit(1)
print("\nall smoke checks passed")
