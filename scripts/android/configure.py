#!/usr/bin/env python3
"""Apply the tracked Android overlay after `tauri android init` (idempotent)."""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ANDROID = ROOT / 'src-tauri/gen/android'
if not (ANDROID / 'app/build.gradle.kts').is_file():
    raise SystemExit('Run npx tauri android init --ci --skip-targets-install first.')

p = ANDROID / 'app/build.gradle.kts'
s = p.read_text()
s = re.sub(r'compileSdk = \d+', 'compileSdk = 36', s)
s = re.sub(r'targetSdk = \d+', 'targetSdk = 36', s)
if 'ndkVersion =' not in s:
    s = s.replace('    compileSdk = 36', '    compileSdk = 36\n    ndkVersion = "28.2.13676358"')
s = re.sub(r'            optimization \{\s*enable = true\s*}', '            isMinifyEnabled = true', s)
# Leave unstripped symbols in target/, not in the installable debug APK.
s = '\n'.join(line for line in s.splitlines() if 'jniLibs.keepDebugSymbols.add' not in line) + '\n'
if 'play-services-auth' not in s:
    s = s.replace('dependencies {', 'dependencies {\n    implementation("com.google.android.gms:play-services-auth:22.0.0")', 1)
if '// BiblioChad upload signing' not in s:
    s += '''
// BiblioChad upload signing: secrets stay outside source control.
val uploadStore = System.getenv("BIBLIOCHAD_ANDROID_KEYSTORE")
if (!uploadStore.isNullOrBlank()) {
    android {
        signingConfigs {
            create("upload") {
                storeFile = file(uploadStore)
                storePassword = System.getenv("BIBLIOCHAD_ANDROID_STORE_PASSWORD")
                    ?: error("BIBLIOCHAD_ANDROID_STORE_PASSWORD is required")
                keyAlias = System.getenv("BIBLIOCHAD_ANDROID_KEY_ALIAS") ?: "upload"
                keyPassword = System.getenv("BIBLIOCHAD_ANDROID_KEY_PASSWORD")
                    ?: error("BIBLIOCHAD_ANDROID_KEY_PASSWORD is required")
            }
        }
        buildTypes.getByName("release").signingConfig = signingConfigs.getByName("upload")
    }
}
'''
p.write_text(s)
p = ANDROID / 'build.gradle.kts'
s = re.sub(r'com.android.tools.build:gradle:[^"]+', 'com.android.tools.build:gradle:8.10.1', p.read_text())
p.write_text(s)
p = ANDROID / 'buildSrc/build.gradle.kts'
p.write_text(re.sub(r'com.android.tools.build:gradle:[^"]+', 'com.android.tools.build:gradle:8.10.1', p.read_text()))
p = ANDROID / 'gradle/wrapper/gradle-wrapper.properties'
p.write_text(re.sub(r'gradle-[\d.]+-bin.zip', 'gradle-8.14.3-bin.zip', p.read_text()))
p = ANDROID / 'gradle.properties'
s = p.read_text()
if 'android.useAndroidX=' not in s:
    s += '\nandroid.useAndroidX=true\n'
p.write_text(s)
p = ANDROID / 'app/src/main/AndroidManifest.xml'
s = p.read_text()
s = re.sub(r'\s*<!-- AndroidTV support -->', '', s)
s = re.sub(r'\s*<uses-feature android:name="android.software.leanback"[^>]+/>', '', s)
s = re.sub(r'\s*<category android:name="android.intent.category.LEANBACK_LAUNCHER"\s*/>', '', s)
if 'android:windowSoftInputMode' not in s:
    s = s.replace('android:launchMode="singleTask"', 'android:launchMode="singleTask"\n            android:windowSoftInputMode="adjustResize"')
if 'android:allowBackup' not in s:
    s = s.replace('<application', '<application\n        android:allowBackup="false"\n        android:fullBackupContent="false"', 1)
p.write_text(s)
dest = ANDROID / 'app/src/main/java/app/bibliochad/reader'
dest.mkdir(parents=True, exist_ok=True)
shutil.copy2(ROOT / 'mobile/android/AndroidBridge.kt', dest / 'AndroidBridge.kt')
# @Command entry points are reflected into by Tauri and must survive R8.
p = ANDROID / 'app/proguard-rules.pro'
s = p.read_text() if p.exists() else ''
rule = '-keep class app.bibliochad.reader.AndroidBridge { *; }'
if rule not in s:
    p.write_text(s + '\n' + rule + '\n-keep class app.bibliochad.reader.**Args { *; }\n')
print('Configured Android API 36, native services, private storage and upload signing.')

# Derive Android launcher assets from the existing BiblioChad icon.
with tempfile.TemporaryDirectory(prefix='bibliochad-icons-') as generated:
    subprocess.run(['npx', 'tauri', 'icon', str(ROOT / 'assets/icon.png'), '--output', generated], cwd=ROOT, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shutil.copytree(Path(generated) / 'android', ANDROID / 'app/src/main/res', dirs_exist_ok=True)
