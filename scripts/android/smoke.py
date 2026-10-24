#!/usr/bin/env python3
"""Smoke-test a debug APK on a fresh English-language Android emulator.

Uses only adb and UI Automator, including the real system document picker.
Existing app data is preserved unless --reset-data is explicitly supplied.
Run scripts/make-test-corpus.py first. Google sign-in requires separate testing
with your registered OAuth project and is deliberately not automated here.
"""
import argparse
from pathlib import Path
import re
import subprocess
import time
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--serial', required=True, help='The emulator/device created for this test')
parser.add_argument('--apk', type=Path, required=True)
parser.add_argument('--reset-data', action='store_true', help='Delete BiblioChad app data ON THIS TEST DEVICE before testing')
parser.add_argument('--out', type=Path, default=Path('dist/android/smoke'))
args = parser.parse_args()
ROOT = Path(__file__).resolve().parents[2]
args.out.mkdir(parents=True, exist_ok=True)


def adb(*command, binary=False):
    return subprocess.check_output(['adb', '-s', args.serial, *command], text=not binary, timeout=60)


def tree():
    adb('shell', 'rm', '-f', '/sdcard/bibliochad-smoke.xml')
    adb('shell', 'uiautomator', 'dump', '/sdcard/bibliochad-smoke.xml')
    return ET.fromstring(adb('shell', 'cat', '/sdcard/bibliochad-smoke.xml'))


def find(label, timeout=30, scope=None):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            root = tree()
        except (subprocess.CalledProcessError, ET.ParseError):
            time.sleep(0.5)
            continue
        nodes = root.iter('node')
        if scope:
            containers = [n for n in root.iter('node') if n.get('resource-id', '').endswith('/' + scope)]
            nodes = (n for container in containers for n in container.iter('node'))
        for node in nodes:
            if label in (node.get('text'), node.get('content-desc')):
                coords = list(map(int, re.findall(r'\d+', node.get('bounds', ''))))
                if len(coords) == 4 and coords[2] > coords[0] and coords[3] > coords[1]:
                    return coords
        time.sleep(0.5)
    raise AssertionError(f'Could not find {label!r}. Use a fresh emulator or explicitly --reset-data.')


def tap(label, scope=None):
    x1, y1, x2, y2 = find(label, scope=scope)
    adb('shell', 'input', 'tap', str((x1 + x2) // 2), str((y1 + y2) // 2))


def screenshot(name):
    (args.out / f'{name}.png').write_bytes(adb('exec-out', 'screencap', '-p', binary=True))


adb('install', '-r', str(args.apk.resolve()))
if args.reset_data:
    adb('shell', 'pm', 'clear', 'app.bibliochad.reader')
adb('shell', 'am', 'start', '-n', 'app.bibliochad.reader/.MainActivity')
try:
    tap('Try a sample book')
    find('Back to library')
    find('Welcome to BiblioChad')
    screenshot('01-epub')
    print('PASS: sample EPUB opens without login', flush=True)
    adb('shell', 'input', 'keyevent', '4')
    find('Library views')
    print('PASS: Android Back returns to library', flush=True)
    corpus = ROOT / 'test-corpus/chad-test.pdf'
    if not corpus.is_file():
        raise AssertionError('Run python3 scripts/make-test-corpus.py before this test.')
    adb('push', str(corpus), '/sdcard/Download/BiblioChad-smoke.pdf')
    tap('Import')
    tap('Show roots')
    tap('Downloads', scope='roots_list')
    tap('BiblioChad-smoke.pdf')
    find("Open Chad's Big PDF")
    tap("Open Chad's Big PDF")
    find('PDF pages')
    find('Page 1 of 40')
    screenshot('02-pdf')
    copies = adb('shell', 'run-as', 'app.bibliochad.reader', 'find', 'files/imports', '-type', 'f')
    assert 'BiblioChad-smoke.pdf' in copies, copies
    print('PASS: content URI import persists a private copy and PDFium renders the PDF', flush=True)
    adb('shell', 'input', 'keyevent', '4')
    find('Library views')
    adb('shell', 'am', 'force-stop', 'app.bibliochad.reader')
    adb('shell', 'am', 'start', '-n', 'app.bibliochad.reader/.MainActivity')
    tap('All books')
    find("Open Chad's Big PDF")
    find('Open Welcome to BiblioChad')
    screenshot('03-restored-library')
    print('PASS: imported and sample books survive app restart', flush=True)
    # Exercise the same context menu through Android's native long-press gesture.
    adb('shell', 'input', 'swipe', '850', '1950', '850', '850', '350')
    x1, y1, x2, y2 = find('Open Welcome to BiblioChad')
    x, y = str((x1 + x2) // 2), str((y1 + y2) // 2)
    adb('shell', 'input', 'swipe', x, y, x, y, '800')
    tap('Add to Collection')
    fields = [n for n in tree().iter('node') if n.get('class') == 'android.widget.EditText']
    assert fields, 'Collection name input was not exposed to accessibility'
    x1, y1, x2, y2 = map(int, re.findall(r'\d+', fields[-1].get('bounds')))
    adb('shell', 'input', 'tap', str((x1 + x2) // 2), str((y1 + y2) // 2))
    adb('shell', 'input', 'text', 'Android%ssmoke')
    screenshot('04-collection-keyboard')
    tap('Create and add')
    tap('Collections')
    tap('Android smoke 1')
    find('Open Welcome to BiblioChad')
    screenshot('05-collection-saved')
    print('PASS: long-press menu and collection creation work with the Android keyboard', flush=True)
finally:
    screenshot('last-state')
