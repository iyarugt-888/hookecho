package io.hookecho.HookEcho

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File

/**
 * Self-update from the CI build (see `self_update.rs`). Rust downloads and checksums the APK into
 * [updatesDir]; [install] hands it to the system package installer, which asks the user to
 * confirm and only accepts an APK signed with the same key as the installed app.
 *
 * Called from Rust over JNI (`platform.rs`, `android_alerts::install_apk` / `update_dir`).
 */
object Updater {
    /** The cache subdirectory the `.updates` FileProvider shares (res/xml/update_paths.xml). */
    @JvmStatic
    fun updatesDir(context: Context): String {
        val dir = File(context.cacheDir, "updates")
        dir.mkdirs()
        return dir.absolutePath
    }

    /** [install]'s answers, mirrored in `platform.rs`. */
    private const val STARTED = 0
    private const val NEEDS_PERMISSION = 1
    private const val DIFFERENT_SIGNER = 2
    private const val OLDER_VERSION = 3

    /**
     * Start the system installer for the APK at [path], after checking it is one Android will
     * accept: Android refuses an update signed with a different key, or older than what is
     * installed, with only "App not installed". Checked here, the app can say why instead.
     * Returns [STARTED]; [NEEDS_PERMISSION] after opening the "install unknown apps" page for
     * HookEcho (installing again after allowing it proceeds); or [DIFFERENT_SIGNER] /
     * [OLDER_VERSION] without starting anything.
     */
    @JvmStatic
    fun install(context: Context, path: String): Int {
        val pm = context.packageManager
        val apk = pm.getPackageArchiveInfo(path, PackageManager.GET_SIGNING_CERTIFICATES)
        val self = pm.getPackageInfo(context.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
        // Unreadable either way: let the installer decide rather than block on a guess.
        val theirs = signers(apk)
        val ours = signers(self)
        if (theirs != null && ours != null && theirs.intersect(ours).isEmpty()) {
            return DIFFERENT_SIGNER
        }
        if (apk != null && apk.longVersionCode < self.longVersionCode) {
            return OLDER_VERSION
        }
        if (!pm.canRequestPackageInstalls()) {
            val settings = Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${context.packageName}"),
            ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(settings)
            return NEEDS_PERMISSION
        }
        val uri = FileProvider.getUriForFile(
            context,
            "${context.packageName}.updates",
            File(path),
        )
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent)
        return STARTED
    }

    /** The signing certificates of a package, including any it rotated from; null if unknown. */
    private fun signers(info: PackageInfo?): Set<String>? {
        val signing = info?.signingInfo ?: return null
        val certs = if (signing.hasMultipleSigners()) {
            signing.apkContentsSigners
        } else {
            signing.signingCertificateHistory
        } ?: return null
        return certs.map { it.toCharsString() }.toSet().takeIf { it.isNotEmpty() }
    }
}
