package dev.patchforge.patchforge_sdk

import android.content.Context
import android.content.pm.ApplicationInfo
import android.util.Log
import io.flutter.FlutterInjector
import java.io.File
import java.io.FileInputStream
import java.security.MessageDigest

internal object PatchForgeBootstrap {
    private const val tag = "PatchForgeBootstrap"
    private const val preferencesName = "FlutterSharedPreferences"
    private const val prefix = "flutter."
    private const val currentPatchKey = "${prefix}patchforge_current_patch"
    private const val previousPatchKey = "${prefix}patchforge_previous_patch"
    private const val pendingPatchKey = "${prefix}patchforge_pending_patch"
    private const val patchStatusKey = "${prefix}patchforge_patch_status"

    @Volatile
    var loadedPatch: Long? = null
        private set

    fun initialize(context: Context) {
        if ((context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0) {
            Log.i(tag, "Skipping AOT patch activation for a debuggable build")
            return
        }

        val patch = selectPatch(context) ?: return
        val library = patchFile(context, patch)

        if (!verifyPatch(context, patch, library)) {
            Log.e(tag, "Patch $patch failed activation integrity checks")
            failPatch(context, patch)
            return
        }

        val preferences = preferences(context)
        val pending = preferences.getLong(pendingPatchKey, 0L)

        if (pending == patch) {
            preferences.edit().putString(patchStatusKey, "booting").commit()
        }

        val argument = "--aot-shared-library-name=${library.canonicalPath}"
        val loader = FlutterInjector.instance().flutterLoader()

        loader.startInitialization(context.applicationContext)
        loader.ensureInitializationComplete(context.applicationContext, arrayOf(argument))
        loadedPatch = patch

        Log.i(tag, "Flutter engine initialized with PatchForge patch $patch")
    }

    private fun selectPatch(context: Context): Long? {
        val preferences = preferences(context)
        val status = preferences.getString(patchStatusKey, null)
        val pending = preferences.getLong(pendingPatchKey, 0L)

        if (status == "booting" && pending > 0L) {
            // A new process started before Dart confirmed the previous boot.
            Log.e(tag, "Patch $pending did not complete its first boot; rolling back")
            failPatch(context, pending)
        }

        val refreshed = preferences(context)
        val refreshedStatus = refreshed.getString(patchStatusKey, null)
        val refreshedPending = refreshed.getLong(pendingPatchKey, 0L)

        if (refreshedStatus == "staged" && refreshedPending > 0L) {
            return refreshedPending
        }

        val current = refreshed.getLong(currentPatchKey, 0L)
        return current.takeIf { it > 0L && patchFile(context, it).isFile }
    }

    private fun verifyPatch(context: Context, patch: Long, library: File): Boolean {
        if (!library.isFile) {
            return false
        }

        val preferences = preferences(context)
        val expectedSize = preferences.getLong(patchSizeKey(patch), -1L)
        val expectedSha256 = preferences.getString(patchSha256Key(patch), null)

        if (expectedSize < 0L || expectedSha256.isNullOrBlank()) {
            return false
        }

        if (library.length() != expectedSize) {
            return false
        }

        return sha256(library).equals(expectedSha256, ignoreCase = true)
    }

    private fun sha256(file: File): String {
        val digest = MessageDigest.getInstance("SHA-256")
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)

        FileInputStream(file).use { input ->
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
        }

        return digest.digest().joinToString("") { "%02x".format(it) }
    }

    private fun failPatch(context: Context, patch: Long) {
        val preferences = preferences(context)
        val previous = preferences.getLong(previousPatchKey, 0L)

        preferences.edit()
            .putLong(currentPatchKey, previous)
            .remove(pendingPatchKey)
            .remove(patchSha256Key(patch))
            .remove(patchSizeKey(patch))
            .putString(patchStatusKey, "failed")
            .commit()

        patchFile(context, patch).parentFile?.deleteRecursively()
    }

    private fun preferences(context: Context) =
        context.getSharedPreferences(preferencesName, Context.MODE_PRIVATE)

    private fun patchFile(context: Context, patch: Long) =
        File(context.filesDir, "patchforge/patches/patch-$patch/libapp.so")

    private fun patchSha256Key(patch: Long) = "${prefix}patchforge_patch_${patch}_sha256"

    private fun patchSizeKey(patch: Long) = "${prefix}patchforge_patch_${patch}_size"
}
