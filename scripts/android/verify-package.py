#!/usr/bin/env python3
"""Check an APK/AAB includes both readers and only 16 KB compatible native code."""
import importlib.util
from pathlib import Path
import sys
import zipfile

spec = importlib.util.spec_from_file_location('android_pdfium', Path(__file__).with_name('pdfium.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
with zipfile.ZipFile(sys.argv[1]) as archive:
    native = [n for n in archive.namelist() if n.endswith('.so')]
    if not native:
        raise SystemExit('No native libraries in package')
    abis = {n.split('/')[-2] for n in native}
    for abi in abis:
        for required in ['libpdfium.so', 'libbibliochad_lib.so']:
            if not any(n.endswith(f'/{abi}/{required}') for n in native):
                raise SystemExit(f'Missing {required} for {abi}')
    for name in native:
        module.check_elf(archive.read(name))
        print(f'16 KB compatible: {name}')
print('Both reader libraries are packaged for every included ABI.')
