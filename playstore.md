# Publishing BiblioChad on Google Play

Last checked against Google's documentation: **22 October 2026**. Follow the
linked official pages and the tasks shown in your own Play Console if Google
changes a requirement. A working build does not guarantee approval: Google
reviews the app, listing, account, permissions, and data practices.

This repository now builds an Android application from the same Tauri/Rust and
Svelte code as the desktop app. The tracked native overlay is in `mobile/android`;
`scripts/android` regenerates and configures the ignored Android Studio project.
The application ID is **`app.bibliochad.reader`**, minimum Android version is
**Android 8 / API 26**, and the build targets **API 36**. The current Android
`versionCode` is **1003** in `src-tauri/tauri.android.conf.json`.

## 1. Decide who is publishing the app

1. Open [Google Play Console](https://play.google.com/console/signup).
2. Choose **Personal** if you are publishing as yourself, or **Organization** for
   a real business/organization. Use accurate identity and contact information.
3. Pay Google's one-time **US$25 registration fee** and complete the identity,
   email, phone, and any device-verification tasks shown in your account.
4. An organization generally needs a **D-U-N-S number**. Gather the business
   records Google requests before starting; do not pick this account type merely
   to avoid personal-account testing requirements.
5. Complete the Android developer verification tasks in Play Console, including
   package registration when requested. Existing Play publishing and Android
   developer verification are related processes; follow the account dashboard.

Sources: [account registration](https://support.google.com/googleplay/android-developer/answer/6112435?hl=en),
[account types](https://support.google.com/googleplay/android-developer/answer/13634885?hl=en),
[developer verification](https://developer.android.com/developer-verification/guides/faq).

## 2. Install the build tools

Install Node.js 22+, Rust, a **JDK 17**, and Android Studio. In Android Studio's
SDK Manager install **Android SDK Platform 36**, **Build-Tools 36.0.0**,
**Platform-Tools**, and **NDK 28.2.13676358**. The generated Gradle project is
configured for AGP 8.10.1 and Gradle 8.14.3 by the overlay script.

Example environment on Linux; replace the paths with your installations:

```bash
export JAVA_HOME=/path/to/jdk-17
export ANDROID_HOME="$HOME/Android/Sdk"
export NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"
rustup target add aarch64-linux-android
npm ci
npm --prefix ui ci
npm run android:init
npm run android:apk
```

The shell scripts use Bash and Python 3. On Windows use an appropriate Bash
shell with the Windows Android SDK/JDK configured, or run the commands in Linux
with a Linux SDK. Do not mix Windows and Linux NDK executables.

The installable **debug** APK is:

```text
src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

Install it on your own phone with `adb install -r <apk-path>`. A debug APK is for
sideloading/testing; it is **not** your Play submission. For an x86_64 emulator,
install the Rust target `x86_64-linux-android`, fetch its PDFium with
`python3 scripts/android/pdfium.py --target x86_64`, then build with
`npx tauri android build --apk --debug --target x86_64 --ci`.

Source: [Tauri Android prerequisites](https://v2.tauri.app/start/prerequisites/).

## 3. Confirm the permanent application identity

Create the app in Play Console: choose the default language, **App**, and the
appropriate free/paid selection. Review the declarations shown by the console.
For the current app, there are no ads or purchases.

Keep `app.bibliochad.reader` as the application ID unless deliberately publishing
a separately named fork. Treat the package name as permanent once published.
Before each later upload, increase `bundle.android.versionCode` in
`src-tauri/tauri.android.conf.json`; do not reuse 1003 for a different upload.
Update the visible version and release notes when shipping a new release.

The generated directory is ignored by Git: keep custom native changes in the
tracked overlay/scripts, then rerun `npm run android:init`. Do not expect manual
edits under `src-tauri/gen/android` to survive regeneration.

Source: [Android app versioning](https://developer.android.com/studio/publish/versioning).

## 4. Create and protect an upload key

Create an upload keystore **outside the repository**:

```bash
keytool -genkeypair -v -keystore "$HOME/bibliochad-upload.jks" \
  -alias upload -keyalg RSA -keysize 2048 -validity 10000
```

Enter the passwords interactively. Keep an encrypted backup of the keystore,
alias, and passwords somewhere you can recover. Never commit them, send them to
a reviewer, or include them in an APK. The repository ignores `.jks` and
`.keystore` files, but that is not a substitute for keeping keys outside it.

Enable **Play App Signing** when Play Console asks. Your upload key signs the
bundle you send to Google; Google's app-signing key signs the APKs users receive.
Their certificate fingerprints are different. Keep track of both.

Source: [Android signing and Play App Signing](https://developer.android.com/studio/publish/app-signing).

## 5. Configure native Android Google Drive authorization

Local EPUB/PDF reading works without Google login. To offer Drive in a public
build, configure your publisher's Google Cloud project:

1. Enable the **Google Drive API**.
2. Configure Google Auth Platform branding, support contact, audience, homepage,
   privacy policy, and verified domains.
3. Create an OAuth client of type **Android**, using `app.bibliochad.reader` and
   the app-signing certificate's **SHA-1** from Play Console's app integrity /
   app-signing page. Menu wording can vary.
4. Register separate Android OAuth credentials in that same project for local
   debug and locally signed release certificates if you test those builds.
   `apksigner verify --print-certs <apk>` shows the certificate on an APK;
   `keytool -list -v -keystore <keystore>` shows a keystore's certificate.
5. During OAuth testing, add the permitted test accounts and use a device with
   Google Play services. Register desktop and Android clients in the same
   Google Cloud project and verify that they can read the same hidden sync file.
   Do not assume an unrelated desktop bring-your-own-client project shares your
   Android app's private Drive storage. Test consent refusal, partial consent, cancellation,
   reconnection, revoked access, and a successful folder crawl/download/sync.

Android uses **Google Identity Services AuthorizationClient** and short-lived
access tokens. Do **not** import the desktop OAuth JSON, embed a web client
secret, use an embedded browser login, or reuse the desktop loopback redirect.
The native SDK handles token grants; BiblioChad holds access tokens in memory.

Sources: [Android authorization](https://developer.android.com/identity/authorization),
[Google APIs setup](https://developers.google.com/identity/protocols/oauth2/native-app).

## 6. Complete OAuth verification separately from Play review

BiblioChad requests:

- `drive.readonly`: enumerate the user's existing folder tree and read books.
- `drive.appdata`: synchronize progress, bookmarks, and highlights in the app's
  hidden Drive app-data folder.

**`drive.readonly` is a restricted scope.** Merely switching the OAuth audience
to production does not complete verification. Submit the required scope
justification, verified-domain details, privacy policy, and a demonstration of
how the app uses the requested data through Google's OAuth verification flow.
Show why existing folder-tree browsing needs this access. Replacing it with
`drive.file` would require changing the app's access model; it is not an
interchangeable scope that automatically exposes every book in a folder.

Restricted-scope verification and any applicable security assessment depend on
your data flow. The current app connects directly from the device to Google and
has no BiblioChad-operated server. Explain that accurately; do not assume an
assessment exemption without Google's confirmation. Adding a backend later can
change the requirements. **Google Play approval and OAuth verification are two
separate approvals.** Do not advertise working public Drive access until both
are ready.

Sources: [Drive scope classifications](https://developers.google.com/workspace/drive/api/guides/api-specific-auth),
[restricted-scope verification](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification).

## 7. Publish a real privacy policy and disclose the actual data flow

Complete [the privacy policy draft](mobile/privacy-policy-template.md) with your
legal name, support email, effective date, and actual privacy procedures. Review
it against your final build. Host it at a publicly accessible **HTTPS** URL that
does not require a login, and enter that URL in Play Console. The app also shows
an Android Settings privacy summary and links to the URL supplied at build time:

```bash
export VITE_BIBLIOCHAD_PRIVACY_URL='https://your-real-domain.example/privacy'
```

Replace the example with a working policy page before a production build. The
signed-bundle helper refuses to run without an HTTPS policy URL.

Describe local book copies, covers, account email, Google authorization,
positions, highlights, notes, and optional direct-to-Drive synchronization.
Explain retention and deletion. Disconnect/wipe does not delete the existing
cloud sync document or original books. Users can clear Android app storage and
manage BiblioChad's hidden app data in Google Drive. Include a usable contact.

Source: [Google Play user-data and privacy policy rules](https://support.google.com/googleplay/android-developer/answer/10144311?hl=en).

## 8. Fill out Data safety and App content carefully

In Play Console complete **App content**, including Data safety, ads, target
audience, content rating, app access, and the privacy-policy URL.

Audit the final code and every SDK. Local-only processing differs from data
transmitted off the device; optional Drive sync still transmits reading data.
Do not select “collects no data” simply because there is no developer server.
Check Google's definitions and exemptions, the Google Play services SDK's
current disclosures, and the precise fields requested by the form. Describe
which features are optional, what is encrypted in transit, why each category is
used, and how deletion works. Never copy another app's answers.

The current app does not create a BiblioChad account. If you later add account
creation, revisit the requirement for an in-app deletion option and a public
web deletion-request route. Disconnecting Google is not deleting a Google
account. Choose the age audience and rating honestly; Chad Mode includes jokes
and is not evidence that the product is designed for children.

Sources: [Data safety](https://support.google.com/googleplay/android-developer/answer/10787469?hl=en),
[account deletion requirements](https://support.google.com/googleplay/android-developer/answer/13327111?hl=en).

## 9. Prepare the listing and reviewer instructions

Prepare the app name, short/full descriptions, the Books & Reference category, contact details, icon,
feature graphic, and real device screenshots. Current asset specifications
include a **512 × 512** app icon and **1024 × 500** feature graphic; follow the
console's current screenshot requirements. Show the actual Android experience,
not desktop screenshots. Only promise features in the submitted build, and
only distribute books/artwork you have rights to distribute.

In App access give reviewers a reproducible path: launch the app, tap **Try a
sample book**, use reading controls, return to Library, and open Settings. The
included sample is original content and requires no account or file permission.
Explain that Drive is optional and give separate instructions for reviewing it.
If a restricted feature needs an account, supply the access resources Google
requests through Play Console; do not include private tokens or keystores.
Avoid workflows blocked by an unapproved OAuth consent screen or inaccessible
2-factor challenge.

Sources: [listing assets](https://support.google.com/googleplay/android-developer/answer/9866151?hl=en),
[prepare your app for review](https://support.google.com/googleplay/android-developer/answer/9859455?hl=en).

## 10. Build the signed Android App Bundle

Set the signing variables in your shell or a secrets manager. Avoid putting
passwords in shell history; for example, enter them with `read -rs` in Bash:

```bash
export BIBLIOCHAD_ANDROID_KEYSTORE="$HOME/bibliochad-upload.jks"
export BIBLIOCHAD_ANDROID_KEY_ALIAS=upload
read -rs -p 'Keystore password: ' BIBLIOCHAD_ANDROID_STORE_PASSWORD; echo
export BIBLIOCHAD_ANDROID_STORE_PASSWORD
read -rs -p 'Key password: ' BIBLIOCHAD_ANDROID_KEY_PASSWORD; echo
export BIBLIOCHAD_ANDROID_KEY_PASSWORD
# VITE_BIBLIOCHAD_PRIVACY_URL must already point to your real policy.
npm run android:bundle
```

Upload the `.aab` from
`src-tauri/gen/android/app/build/outputs/bundle/universalRelease/`.
An AAB is not directly installable. Use a Play test track to install the APKs
Google generates. Check its package, version, signing and native libraries:

```bash
jarsigner -verify -verbose -certs path/to/app-universal-release.aab
python3 scripts/android/verify-package.py path/to/app-universal-release.aab
```

Source: [Android App Bundles](https://developer.android.com/guide/app-bundle).

## 11. Check technical compatibility before uploading

For a new phone app, Google's current requirement is **target API 36 or later**;
recheck it when uploading. This project sets API 36 and packages ARM64 native
libraries. Both Rust and PDFium must support **16 KB memory pages**. NDK r28 is
used, the pinned PDFium binary is checked, and the package verifier checks every
included `.so`. For an APK also run:

```bash
"$ANDROID_HOME/build-tools/36.0.0/zipalign" -c -P 16 4 path/to/app.apk
```

Passing ELF/ZIP checks is not a substitute for testing on a 16 KB device or
emulator. Test API 26 and recent Android versions, small and large screens,
rotation, font scaling, keyboard/insets, Back, background/restore, airplane mode,
local EPUB/PDF imports from different document providers, PDF rendering/search,
EPUB highlights, collections, export, and Drive account changes. Keep Android
System WebView current. On a dedicated English-language emulator, the repository
also provides a native file-picker/reader smoke test (the reset flag deliberately
clears only this test app's data on the specified device):

```bash
python3 scripts/make-test-corpus.py
python3 scripts/android/smoke.py --serial emulator-5554 \
  --apk path/to/x86_64-debug.apk --reset-data
``` Investigate crashes and ANRs before submission.

Sources: [target API requirements](https://developer.android.com/google/play/requirements/target-sdk),
[16 KB compatibility and device testing](https://developer.android.com/guide/practices/page-sizes).

## 12. Run internal and required closed testing

Upload the signed AAB to **Internal testing**, add testers, publish that test
release, and install through its opt-in link. Confirm that native Drive login
works with the **Play app-signing** certificate, not just your local debug key.
Check the pre-launch report and repair crashes, accessibility issues, or layout
failures it identifies. Ask testers to exercise real reading and sync flows.

For **personal developer accounts created after 13 November 2023**, Google
currently requires a closed test with **at least 12 opted-in testers for at least
14 consecutive days** before you can apply for production access. Internal
track testing alone does not satisfy that requirement. Keep testers opted in,
gather meaningful feedback, record the issues found and fixes made, then answer
the production-access application honestly. Reaching the numbers alone does
not guarantee access. Account-specific tasks in Play Console remain decisive.

Sources: [testing requirements](https://support.google.com/googleplay/android-developer/answer/14151465?hl=en),
[test tracks](https://support.google.com/googleplay/android-developer/answer/9845334?hl=en),
[pre-launch reports](https://support.google.com/googleplay/android-developer/answer/9844487?hl=en).

## 13. Submit the production release and respond to review

1. Confirm production access, OAuth verification, listing, policy URLs, App
   content answers, app access instructions, and signed AAB are all ready.
2. Create a Production release. Select the tested bundle or upload a new one
   with a higher version code; provide useful release notes and countries.
3. Resolve every blocking console error. Review Publishing overview and send
   the changes for review. Use managed publishing if you want manual control
   over when approved changes go live.
4. Allow time for review; do not promise a fixed approval date. Check both the
   Play Console and your registered support inbox.
5. If rejected, read the specific policy and affected screen/data flow. Fix the
   app and/or declaration, increment the version code for a changed binary,
   retest, and resubmit. Appeal through the provided route when you have
   concrete evidence the decision is mistaken.
6. After approval, monitor Android vitals, feedback and OAuth errors. Keep the
   app and its policy declarations current as dependencies or data flows change.

Sources: [publishing overview and managed publishing](https://support.google.com/googleplay/android-developer/answer/9859654?hl=en),
[Android vitals](https://developer.android.com/topic/performance/vitals).

## Final submission checklist

- [ ] Verified developer identity, contacts, package registration and account tasks.
- [ ] Stable package name, higher version code, protected upload key, Play App Signing.
- [ ] Native Android OAuth credentials for the certificates actually used.
- [ ] Drive scope verification completed; consent and revocation tested on a real account.
- [ ] Real public privacy policy linked in the app and Play listing; accurate Data safety.
- [ ] Honest audience/rating/ads/app-access answers and current screenshots.
- [ ] Signed AAB; native and ZIP alignment checked; required device tests completed.
- [ ] Sample book and offline reading work without login; reviewers can reach all features.
- [ ] Required closed testing and production access completed for your account.
- [ ] Console blockers and pre-launch failures resolved before review submission.
