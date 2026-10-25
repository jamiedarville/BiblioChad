# BiblioChad privacy policy — complete before publishing

**Developer:** [your legal developer name]  
**Contact:** [your support email]  
**Effective date:** [date]

Replace these placeholders, review the final SDK data disclosures, and publish
this policy at a public HTTPS URL. Set `VITE_BIBLIOCHAD_PRIVACY_URL` when building
for Google Play. This draft describes the current code, not future features.

BiblioChad is an EPUB and PDF reader. You can use it without creating an account.
Imported books, cover images, local metadata, collections, reading positions,
bookmarks, highlights, and notes are kept in the app's private device storage.
Android backup of this app's data is disabled.

Google Drive connection is optional. If you connect it, Google Play services
handles account selection and authorization. Access tokens are held in memory;
BiblioChad does not embed an OAuth client secret or keep a refresh token on
Android. Google manages the underlying grant and token cache.

With your authorization, the app reads Drive folder and file metadata, downloads
books and covers, and uses your Drive account email to identify the connection.
Reading positions, bookmarks, and highlights can be synced directly to the
hidden BiblioChad app-data folder in your Google Drive. Your ebook files are not
modified. No BiblioChad-operated server receives your books or reading data.
Google processes data under its own privacy policy and service terms.

The current app does not include advertising, analytics, crash-reporting uploads,
tracking identifiers, payments, or a BiblioChad account system. It makes network
requests for the Google features you enable. Review Google Play services SDK
disclosures separately when completing the Play Data safety form.

Disconnect Drive in Settings to stop synchronization. When online, disconnect
also requests revocation of Google's authorization grant. You can always revoke
access independently in your Google Account's connected-app settings. A local
wipe or uninstall does not delete the Google Drive sync file. Remove BiblioChad's
hidden app data through Google Drive Settings → Manage apps if you want to delete
that cloud copy. Original Drive books are not deleted.

Use Android Settings → Apps → BiblioChad → Storage → Clear storage, or uninstall,
to delete the app's local copies and reading data. Removing an imported book in
the app deletes its private imported copy; the document originally selected in
the system picker is not deleted. Contact [your support email] with privacy
questions. [Describe any applicable retention, rights request, and regional
privacy procedures for your actual publisher and distribution.]
