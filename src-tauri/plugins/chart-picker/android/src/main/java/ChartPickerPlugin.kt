package com.cnmdbook.chartpicker

import android.app.Activity
import android.content.Intent
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.util.concurrent.Executors

@TauriPlugin
class ChartPickerPlugin(private val activity: Activity) : Plugin(activity) {
    private val importer = Executors.newSingleThreadExecutor()

    @Command
    fun prepareAssets(invoke: Invoke) {
        importer.execute {
            try {
                val version = activity.packageManager.getPackageInfo(activity.packageName, 0).lastUpdateTime
                val dir = File(activity.filesDir, "render-assets-$version")
                val ready = File(dir, ".ready")
                if (!ready.exists()) {
                    fun copy(source: String, target: File) {
                        val entries = activity.assets.list(source) ?: emptyArray()
                        if (entries.isNotEmpty()) {
                            check(target.isDirectory || target.mkdirs())
                            entries.forEach { copy("$source/$it", File(target, it)) }
                        } else {
                            activity.assets.open(source).use { input ->
                                target.outputStream().use { output -> input.copyTo(output) }
                            }
                        }
                    }
                    copy("assets", dir)
                    check(File(dir, "font.ttf").isFile && File(dir, "ending.mp3").isFile)
                    ready.writeText("ready")
                }
                invoke.resolve(JSObject().put("path", dir.absolutePath))
            } catch (error: Exception) {
                invoke.reject(error.message ?: "Failed to prepare renderer assets")
            }
        }
    }

    @Command
    fun pick(invoke: Invoke) {
        // GET_CONTENT can be intercepted by the Android photo picker. Charts
        // are documents, including archives whose extensions have no MIME type.
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType("*/*")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        startActivityForResult(invoke, intent, "picked")
    }

    @Command
    fun pickFolder(invoke: Invoke) {
        // A document tree has no filesystem path. Import its contents while
        // the result grant is valid, preserving the chart's relative paths.
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        startActivityForResult(invoke, intent, "folderPicked")
    }

    @ActivityCallback
    fun folderPicked(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_CANCELED) {
            invoke.resolve(JSObject().put("path", org.json.JSONObject.NULL))
            return
        }
        val tree = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || tree == null) {
            invoke.reject("Failed to select a chart folder")
            return
        }
        importer.execute {
            var importDir: File? = null
            try {
                val resolver = activity.contentResolver
                val rootId = DocumentsContract.getTreeDocumentId(tree)
                val rootUri = DocumentsContract.buildDocumentUriUsingTree(tree, rootId)
                val rootName = resolver.query(
                    rootUri, arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null
                )?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                } ?: "chart"
                fun checkedName(name: String?): String {
                    check(!name.isNullOrBlank() && name != "." && name != ".." &&
                        name.none { it == '/' || it == '\\' || it == '\u0000' }) {
                        "The selected folder contains an invalid file name"
                    }
                    return name!!
                }
                val dir = File(activity.cacheDir, "charts/${java.util.UUID.randomUUID()}")
                importDir = dir
                check(dir.mkdirs()) { "Cannot create the chart import directory" }
                val target = File(dir, checkedName(rootName))
                val ancestors = mutableSetOf<String>()
                fun copyFolder(documentId: String, destination: File) {
                    check(ancestors.add(documentId)) { "The selected folder contains a directory cycle" }
                    try {
                        check(destination.mkdir()) { "Cannot create a chart folder" }
                        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, documentId)
                        resolver.query(children, arrayOf(
                            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                            DocumentsContract.Document.COLUMN_MIME_TYPE
                        ), null, null, null)?.use { cursor ->
                            while (cursor.moveToNext()) {
                                val childId = cursor.getString(0)
                                val file = File(destination, checkedName(cursor.getString(1)))
                                check(!file.exists()) { "The selected folder contains duplicate file names" }
                                if (cursor.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) {
                                    copyFolder(childId, file)
                                } else {
                                    val childUri = DocumentsContract.buildDocumentUriUsingTree(tree, childId)
                                    resolver.openInputStream(childUri)?.use { input ->
                                        file.outputStream().use { output -> input.copyTo(output) }
                                    } ?: error("A file in the selected folder cannot be read")
                                }
                            }
                        } ?: error("The selected folder cannot be read")
                    } finally {
                        ancestors.remove(documentId)
                    }
                }
                copyFolder(rootId, target)
                invoke.resolve(JSObject().put("path", target.absolutePath))
            } catch (error: Exception) {
                importDir?.deleteRecursively()
                invoke.reject(error.message ?: "Failed to import the chart folder")
            }
        }
    }

    @ActivityCallback
    fun picked(invoke: Invoke, result: ActivityResult) {
        val uri = result.data?.data
        if (result.resultCode == Activity.RESULT_CANCELED || uri == null) {
            invoke.resolve(JSObject().put("path", org.json.JSONObject.NULL))
            return
        }
        if (result.resultCode != Activity.RESULT_OK) {
            invoke.reject("Failed to select a chart")
            return
        }
        // Copy while the activity result's read grant is valid, off the UI
        // thread. Each import gets a new directory; reselecting an edited file
        // cannot overwrite a chart a queued render is still reading.
        importer.execute {
            var target: File? = null
            try {
                val resolver = activity.contentResolver
                val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                } ?: "chart.pez"
                val dir = File(activity.cacheDir, "charts/${java.util.UUID.randomUUID()}")
                check(dir.mkdirs()) { "Cannot create the chart import directory" }
                val safeName = name.replace(Regex("[\\\\/:\\x00]"), "_").ifBlank { "chart.pez" }
                val file = File(dir, safeName)
                target = file
                resolver.openInputStream(uri)?.use { input ->
                    file.outputStream().use { output -> input.copyTo(output) }
                } ?: error("The selected document cannot be read")
                invoke.resolve(JSObject().put("path", file.absolutePath))
            } catch (error: Exception) {
                target?.parentFile?.deleteRecursively()
                invoke.reject(error.message ?: "Failed to import the chart")
            }
        }
    }
}
