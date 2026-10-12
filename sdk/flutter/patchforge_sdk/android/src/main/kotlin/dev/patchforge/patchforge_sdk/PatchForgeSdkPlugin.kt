package dev.patchforge.patchforge_sdk

import android.content.Context
import android.os.Build
import android.os.Process
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.util.zip.ZipFile

class PatchForgeSdkPlugin : FlutterPlugin, MethodChannel.MethodCallHandler {
    private lateinit var context: Context
    private lateinit var channel: MethodChannel

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        context = binding.applicationContext
        channel = MethodChannel(binding.binaryMessenger, "dev.patchforge/base_artifact")
        channel.setMethodCallHandler(this)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "getLoadedPatch" -> {
                result.success(PatchForgeBootstrap.loadedPatch)
                return
            }
            "getArchitecture" -> {
                result.success(currentProcessArchitecture())
                return
            }
            "locateBaseArtifact" -> Unit
            else -> {
                result.notImplemented()
                return
            }
        }

        val architecture = call.argument<String>("architecture")

        if (architecture.isNullOrBlank()) {
            result.error("invalid-architecture", "architecture is required", null)
            return
        }

        if (!Build.SUPPORTED_ABIS.contains(architecture)) {
            result.error(
                "unsupported-architecture",
                "Requested $architecture; device supports ${Build.SUPPORTED_ABIS.joinToString()}",
                null,
            )
            return
        }

        try {
            result.success(locateLibApp(architecture).absolutePath)
        } catch (error: Exception) {
            result.error("base-artifact-not-found", error.message, null)
        }
    }

    private fun currentProcessArchitecture(): String? {
        val processAbis = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            if (Process.is64Bit()) Build.SUPPORTED_64_BIT_ABIS else Build.SUPPORTED_32_BIT_ABIS
        } else {
            Build.SUPPORTED_ABIS
        }

        return processAbis.firstOrNull {
            it == "arm64-v8a" || it == "armeabi-v7a" || it == "x86_64"
        }
    }

    private fun locateLibApp(architecture: String): File {
        val extracted = File(context.applicationInfo.nativeLibraryDir, "libapp.so")

        if (extracted.isFile) {
            return extracted
        }

        val apkPaths = buildList {
            add(context.applicationInfo.sourceDir)
            context.applicationInfo.splitSourceDirs?.let { addAll(it) }
        }
        val entryName = "lib/$architecture/libapp.so"
        val destinationDirectory = File(context.cacheDir, "patchforge/base/$architecture")
        val destination = File(destinationDirectory, "libapp.so")

        destinationDirectory.mkdirs()

        for (apkPath in apkPaths) {
            ZipFile(apkPath).use { apk ->
                val entry = apk.getEntry(entryName) ?: return@use
                val temporary = File(destinationDirectory, "libapp.so.tmp")

                apk.getInputStream(entry).use { input ->
                    temporary.outputStream().use { output -> input.copyTo(output) }
                }

                if (destination.exists() && !destination.delete()) {
                    throw IllegalStateException("Could not replace cached libapp.so")
                }

                if (!temporary.renameTo(destination)) {
                    throw IllegalStateException("Could not finalize cached libapp.so")
                }

                return destination
            }
        }

        throw IllegalStateException("$entryName was not found in the installed APK")
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        channel.setMethodCallHandler(null)
    }
}
