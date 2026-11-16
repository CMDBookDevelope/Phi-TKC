package com.cnmdbook.chartpicker

import android.Manifest
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.annotation.Keep
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Called on the native render worker, independently of the WebView bridge. */
@Keep
object VideoPublisher {
    @JvmStatic
    external fun initialize(applicationContext: Context)

    @JvmStatic
    fun publishVideo(context: Context, sourcePath: String, displayName: String): String {
        val source = File(sourcePath)
        check(source.isFile && source.length() > 0) { "The rendered video is missing or empty: $sourcePath" }
        val safeName = displayName.map { if (it == '/' || it == '\\' || it == '\u0000') '_' else it }
            .joinToString("").ifBlank { "output.mp4" }
        val name = if (safeName.endsWith(".mp4", ignoreCase = true)) safeName else "$safeName.mp4"
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            publishScoped(context, source, name)
        } else {
            publishLegacy(context, source, name)
        }
    }

    private fun publishScoped(context: Context, source: File, name: String): String {
        val resolver = context.contentResolver
        val values = ContentValues().apply {
            put(MediaStore.Video.Media.DISPLAY_NAME, name)
            put(MediaStore.Video.Media.MIME_TYPE, "video/mp4")
            put(MediaStore.Video.Media.RELATIVE_PATH, "${Environment.DIRECTORY_MOVIES}/Phi-TKC")
            put(MediaStore.Video.Media.IS_PENDING, 1)
        }
        val collection = MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val uri = checkNotNull(resolver.insert(collection, values)) { "Cannot create a video in the media library" }
        try {
            // Close the destination before exposing it to gallery applications.
            val copied = checkNotNull(resolver.openOutputStream(uri, "w")) { "Cannot open the media library video" }
                .use { output ->
                    source.inputStream().use { input -> input.copyTo(output).also { output.flush() } }
                }
            check(copied == source.length()) { "The media library video was not copied completely" }
            val published = ContentValues().apply { put(MediaStore.Video.Media.IS_PENDING, 0) }
            check(resolver.update(uri, published, null, null) == 1) { "Cannot publish the completed media library video" }
            return uri.toString()
        } catch (error: Exception) {
            runCatching { resolver.delete(uri, null, null) }.onFailure { error.addSuppressed(it) }
            throw IllegalStateException("Cannot save video to the media library; original video: ${source.absolutePath}", error)
        }
    }

    @Suppress("DEPRECATION")
    private fun publishLegacy(context: Context, source: File, name: String): String {
        check(context.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED) {
            "Storage permission is required on Android 9 and earlier; original video: ${source.absolutePath}"
        }
        val movies = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MOVIES), "Phi-TKC")
        check(movies.isDirectory || movies.mkdirs()) { "Cannot create the public video directory" }
        var target = File(movies, name)
        var suffix = 0
        while (!target.createNewFile()) {
            suffix += 1
            target = File(movies, "${name.removeSuffix(".mp4")} ($suffix).mp4")
        }
        val scanLock = Any()
        var mediaUri: Uri? = null
        var failed = false
        try {
            source.inputStream().use { input -> target.outputStream().use { output -> input.copyTo(output) } }
            check(target.length() == source.length()) { "The public video was not copied completely" }
            val scanned = CountDownLatch(1)
            MediaScannerConnection.scanFile(context, arrayOf(target.absolutePath), arrayOf("video/mp4")) { _, uri ->
                synchronized(scanLock) {
                    if (failed) {
                        // A scan may complete after the worker timed out.
                        uri?.let { runCatching { context.contentResolver.delete(it, null, null) } }
                    } else {
                        mediaUri = uri
                    }
                    scanned.countDown()
                }
            }
            check(scanned.await(30, TimeUnit.SECONDS)) { "Timed out waiting for the video to enter the media library" }
            return checkNotNull(mediaUri) { "The public video could not be indexed by the media library" }.toString()
        } catch (error: Exception) {
            synchronized(scanLock) {
                failed = true
                mediaUri
            }?.let { uri ->
                runCatching { context.contentResolver.delete(uri, null, null) }.onFailure { error.addSuppressed(it) }
            }
            target.delete()
            throw IllegalStateException("Cannot save video to the media library; original video: ${source.absolutePath}", error)
        }
    }
}
