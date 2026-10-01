package io.hookecho.HookEcho

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
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

    /**
     * Start the system installer for the APK at [path]. Returns false, after opening the
     * "install unknown apps" page for HookEcho, when the user has not yet allowed this app to
     * install packages; installing again after allowing it proceeds.
     */
    @JvmStatic
    fun install(context: Context, path: String): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            !context.packageManager.canRequestPackageInstalls()
        ) {
            val settings = Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${context.packageName}"),
            ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(settings)
            return false
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
        return true
    }
}
