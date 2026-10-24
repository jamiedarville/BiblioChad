#!/usr/bin/env python3
"""Fetch pinned PDFium binaries and reject ELF segments incompatible with 16 KB pages."""
from pathlib import Path
import argparse
import hashlib
import io
import shutil
import struct
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
VERSION = '8086'
BINARIES = {
    'aarch64': ('arm64', 'arm64-v8a', 'f6ea29495d64795b9af61e41768030795b45f82a21415498ee4ab2c61e135756'),
    'x86_64': ('x64', 'x86_64', '772870e7ed846097d779aa0163f8abfd08a13257e16edf0e1cd1a364f11c8c3c'),
}


def check_elf(data):
    if data[:6] != b'\x7fELF\x02\x01':
        raise ValueError('Expected a little-endian 64-bit ELF library')
    phoff = struct.unpack_from('<Q', data, 32)[0]
    size, count = struct.unpack_from('<HH', data, 54)
    loads = 0
    for index in range(count):
        values = struct.unpack_from('<IIQQQQQQ', data, phoff + size * index)
        if values[0] == 1:
            loads += 1
            if values[7] < 16384 or (values[2] - values[3]) % 16384:
                raise ValueError('A native LOAD segment is not 16 KB compatible')
    if not loads:
        raise ValueError('ELF has no LOAD segments')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=BINARIES, action='append')
    parser.add_argument('--check', type=Path, help='Only validate an existing native library')
    args = parser.parse_args()
    if args.check:
        check_elf(args.check.read_bytes())
        print(f'16 KB compatible: {args.check}')
        return
    for target in args.target or ['aarch64']:
        platform, abi, expected = BINARIES[target]
        dest = ROOT / 'src-tauri/gen/android/app/src/main/jniLibs' / abi
        dest.mkdir(parents=True, exist_ok=True)
        req = urllib.request.Request(
            f'https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F{VERSION}/pdfium-android-{platform}.tgz',
            headers={'User-Agent': 'BiblioChad-build'})
        archive = urllib.request.urlopen(req, timeout=120).read()
        if hashlib.sha256(archive).hexdigest() != expected:
            raise SystemExit('PDFium checksum mismatch; refusing to package it.')
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as tar:
            lib = tar.extractfile('lib/libpdfium.so').read()
            check_elf(lib)
            (dest / 'libpdfium.so').write_bytes(lib)
            license_file = tar.extractfile('LICENSE').read()
        assets = ROOT / 'src-tauri/gen/android/app/src/main/assets'
        assets.mkdir(parents=True, exist_ok=True)
        (assets / 'PDFIUM_LICENSE.txt').write_bytes(license_file)
        shutil.copy2(ROOT / 'THIRD_PARTY_NOTICES.md', assets)
        print(f'PDFium {VERSION}: {abi}, checksum and 16 KB segments verified')


if __name__ == '__main__':
    main()
