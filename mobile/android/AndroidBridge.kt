package app.bibliochad.reader

import android.app.Activity
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.ActivityResult
import androidx.activity.result.IntentSenderRequest
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.auth.api.identity.ClearTokenRequest
import com.google.android.gms.auth.api.identity.Identity
import com.google.android.gms.auth.api.identity.RevokeAccessRequest
import com.google.android.gms.common.api.Scope
import java.io.File
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.Executors

@InvokeArg class AuthorizeArgs { var interactive: Boolean = false; var invalidate: Boolean = false }
@InvokeArg class DocumentArgs { lateinit var uri: String; var text: String = "" }

@TauriPlugin
class AndroidBridge(private val activity: Activity) : Plugin(activity) {
    private val client by lazy { Identity.getAuthorizationClient(activity) }
    private val worker = Executors.newSingleThreadExecutor()
    private var lastToken: String? = null
    private var refreshInsets: (() -> Unit)? = null
    private val scopes = listOf(
        Scope("https://www.googleapis.com/auth/drive.readonly"),
        Scope("https://www.googleapis.com/auth/drive.appdata")
    )

    override fun load(webView: WebView) {
        // API 35+ edge-to-edge windows can overlay the IME without resizing the
        // WebView. Tell the UI the usable height so forms stay above the keyboard.
        var lastViewport = ""
        ViewCompat.setOnApplyWindowInsetsListener(webView) { _, insets ->
            webView.post {
                val location = IntArray(2)
                webView.getLocationInWindow(location)
                val usable = if (Build.VERSION.SDK_INT >= 30) {
                    val bottom = maxOf(insets.getInsets(WindowInsetsCompat.Type.ime()).bottom,
                        insets.getInsets(WindowInsetsCompat.Type.systemBars()).bottom)
                    (activity.windowManager.currentWindowMetrics.bounds.height() - bottom - location[1])
                        .coerceIn(0, webView.height)
                } else webView.height
                val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
                val signature = "$usable:${bars.top}:${bars.left}:${bars.right}:${webView.width}"
                if (usable > 0 && signature != lastViewport) {
                    lastViewport = signature
                    val density = webView.resources.displayMetrics.density
                    val cssHeight = usable / density
                    val top = (bars.top - location[1]).coerceAtLeast(0) / density
                    val left = (bars.left - location[0]).coerceAtLeast(0) / density
                    val right = bars.right / density
                    webView.evaluateJavascript(
                        "document.documentElement.style.setProperty('--android-visible-height','${cssHeight}px');document.documentElement.style.setProperty('--android-safe-top','${top}px');document.documentElement.style.setProperty('--android-safe-left','${left}px');document.documentElement.style.setProperty('--android-safe-right','${right}px');window.dispatchEvent(new Event('android-viewport'))", null)
                }
            }
            insets
        }
        refreshInsets = { lastViewport = ""; ViewCompat.requestApplyInsets(webView) }
        ViewCompat.requestApplyInsets(webView)
        (activity as AppCompatActivity).onBackPressedDispatcher.addCallback(activity,
            object : OnBackPressedCallback(true) {
                override fun handleOnBackPressed() {
                    webView.evaluateJavascript("window.dispatchEvent(new Event('android-back'))", null)
                }
            })
    }

    @Command
    fun authorize(invoke: Invoke) {
        val args = invoke.parseArgs(AuthorizeArgs::class.java)
        activity.runOnUiThread {
            val request = {
                client.authorize(AuthorizationRequest.builder().setRequestedScopes(scopes).build())
                    .addOnSuccessListener { result ->
                        if (result.hasResolution()) {
                            if (!args.interactive) {
                                invoke.reject("Reconnect Google Drive to grant access.", "reauth")
                            } else {
                                val pending = result.pendingIntent
                                if (pending == null) invoke.reject("Google sign-in did not return a resolution.")
                                else startIntentSenderForResult(invoke,
                                    IntentSenderRequest.Builder(pending.intentSender).build(), "authorized")
                            }
                        } else deliverToken(invoke, result)
                    }
                    .addOnFailureListener { invoke.reject("Google authorization failed: ${it.message}") }
            }
            val old = lastToken
            if (args.invalidate && old != null) {
                client.clearToken(ClearTokenRequest.builder().setToken(old).build())
                    .addOnSuccessListener { lastToken = null; request() }
                    .addOnFailureListener { invoke.reject("Could not renew Google authorization: ${it.message}") }
            } else request()
        }
    }

    @ActivityCallback
    fun authorized(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode != Activity.RESULT_OK || result.data == null) {
            invoke.reject("Google sign-in was cancelled.")
            return
        }
        try { deliverToken(invoke, client.getAuthorizationResultFromIntent(result.data!!)) }
        catch (e: Exception) { invoke.reject("Google authorization failed: ${e.message}") }
    }

    private fun deliverToken(invoke: Invoke, result: AuthorizationResult) {
        if (!result.grantedScopes.containsAll(scopes.map { it.scopeUri })) {
            invoke.reject("Both Drive library and app-data permissions are required for Drive sync.", "reauth")
            return
        }
        val token = result.accessToken
        if (token.isNullOrBlank()) { invoke.reject("Google did not return an access token.", "reauth"); return }
        lastToken = token
        invoke.resolve(JSObject().apply { put("token", token) })
    }

    @Command
    fun revoke(invoke: Invoke) {
        activity.runOnUiThread {
            client.revokeAccess(RevokeAccessRequest.builder().setScopes(scopes).build())
                .addOnSuccessListener { lastToken = null; invoke.resolve(JSObject()) }
                .addOnFailureListener { invoke.reject("Could not revoke Google access: ${it.message}") }
        }
    }

    @Command
    fun importDocument(invoke: Invoke) {
        val args = invoke.parseArgs(DocumentArgs::class.java)
        worker.execute {
            var staged: File? = null
            try {
                val uri = Uri.parse(args.uri)
                require(uri.scheme == "content") { "Choose a document through the Android file picker." }
                val name = activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                } ?: "book.epub"
                val safeName = name.substringAfterLast('/').substringAfterLast('\\').take(180)
                require(safeName.endsWith(".epub", true) || safeName.endsWith(".pdf", true)) { "Choose an EPUB or PDF document." }
                val imports = File(activity.filesDir, "imports").apply { mkdirs() }
                staged = File(imports, ".${UUID.randomUUID()}.part")
                val digest = MessageDigest.getInstance("SHA-256")
                activity.contentResolver.openInputStream(uri)?.use { input ->
                    staged!!.outputStream().use { output ->
                        val buffer = ByteArray(65536)
                        var total = 0L
                        while (true) {
                            val count = input.read(buffer)
                            if (count < 0) break
                            total += count
                            require(total <= 1024L * 1024 * 1024) { "Books must be smaller than 1 GB." }
                            digest.update(buffer, 0, count)
                            output.write(buffer, 0, count)
                        }
                    }
                } ?: error("The document provider could not open this file.")
                val hash = digest.digest().joinToString("") { "%02x".format(it.toInt() and 255) }
                val dir = File(imports, hash).apply { mkdirs() }
                val dest = File(dir, safeName)
                if (dest.exists()) staged!!.delete()
                else check(staged!!.renameTo(dest)) { "Could not store the imported book." }
                invoke.resolve(JSObject().apply { put("path", dest.absolutePath) })
            } catch (e: Exception) {
                staged?.delete()
                invoke.reject("Could not import document: ${e.message}")
            }
        }
    }

    @Command
    fun writeDocument(invoke: Invoke) {
        val args = invoke.parseArgs(DocumentArgs::class.java)
        worker.execute {
            try {
                val uri = Uri.parse(args.uri)
                require(uri.scheme == "content") { "Choose an export destination through the Android file picker." }
                activity.contentResolver.openOutputStream(uri, "wt")?.use {
                    it.write(args.text.toByteArray(Charsets.UTF_8))
                } ?: error("The document provider could not open the destination.")
                invoke.resolve(JSObject())
            } catch (e: Exception) { invoke.reject("Could not export highlights: ${e.message}") }
        }
    }

    @Command
    fun refreshViewport(invoke: Invoke) {
        activity.runOnUiThread { refreshInsets?.invoke(); invoke.resolve(JSObject()) }
    }

    @Command
    fun backgroundApp(invoke: Invoke) {
        activity.runOnUiThread { activity.moveTaskToBack(true); invoke.resolve(JSObject()) }
    }
}
