package com.cnmdbook.chartpicker

import android.app.Activity
import android.graphics.RectF
import android.hardware.DataSpace
import android.hardware.HardwareBuffer
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Display
import android.view.SurfaceControl
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import androidx.annotation.RequiresApi
import java.time.Duration
import java.util.concurrent.Executors

@RequiresApi(33)
internal class HdrPreviewSurface private constructor(
    activity: Activity,
    private val session: Long,
    private val width: Int,
    private val height: Int,
    private val format: Format,
) : SurfaceHolder.Callback {
    val view = SurfaceView(activity)
    var onFailure: () -> Unit = {}
    var active = false
        private set

    private var layer: SurfaceControl? = null
    private var failed = false
    private var generation = 0
    private val available = ArrayDeque<HardwareBuffer>()

    init {
        view.holder.addCallback(this)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        if (failed) return
        try {
            if (!supportsDisplay(view.display, format)) {
                useSdr("display does not advertise the selected HDR transfer")
                return
            }
            val parent = checkNotNull(view.surfaceControl)
            layer = SurfaceControl.Builder()
                .setParent(parent)
                .setName("Phi-TKC HDR preview")
                .setBufferSize(width, height)
                .setHidden(true)
                .setOpaque(true)
                .build()
            repeat(3) {
                available.add(HardwareBuffer.create(width, height, HardwareBuffer.RGBA_1010102, 1, USAGE))
            }
            PreviewBridge.setHdr(session, 1)
            Log.i(TAG, "HDR layer prepared: ${Build.MANUFACTURER} ${Build.MODEL}, ${format.name}")
        } catch (error: RuntimeException) {
            useSdr(error.toString())
        }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) = Unit

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        releaseLayer()
    }

    fun present(rect: RectF): Int {
        if (failed) return FALLBACK
        val current = layer ?: return FALLBACK
        if (rect.isEmpty) return FALLBACK
        val buffer = available.removeFirstOrNull() ?: return 0
        val frameGeneration = generation
        return try {
            when (val status = PreviewBridge.readHdrFrame(session, buffer)) {
                1 -> {
                    SurfaceControl.Transaction().use { transaction ->
                        transaction.setDataSpace(current, format.dataSpace)
                            .setPosition(current, rect.left, rect.top)
                            .setScale(current, rect.width() / width, rect.height() / height)
                            .setVisibility(current, true)
                            .setBuffer(current, buffer, null) { fence ->
                                releaseExecutor.execute {
                                    val ready = fence.use { !it.isValid || it.await(Duration.ofSeconds(1)) }
                                    mainHandler.post {
                                        if (generation == frameGeneration && ready) {
                                            available.addLast(buffer)
                                        } else {
                                            buffer.close()
                                            if (generation == frameGeneration) useSdr("buffer release fence did not signal")
                                        }
                                    }
                                }
                            }.apply()
                    }
                    active = true
                    1
                }
                FALLBACK -> {
                    buffer.close()
                    useSdr("could not write the 10-bit hardware buffer")
                    FALLBACK
                }
                else -> {
                    available.addFirst(buffer)
                    status
                }
            }
        } catch (error: RuntimeException) {
            buffer.close()
            useSdr(error.toString())
            FALLBACK
        }
    }

    private fun useSdr(reason: String) {
        Log.w(TAG, "${Build.MANUFACTURER} ${Build.MODEL}: SDR fallback: $reason")
        failed = true
        releaseLayer()
        view.visibility = View.GONE
        onFailure()
    }

    private fun releaseLayer() {
        PreviewBridge.setHdr(session, 0)
        active = false
        generation += 1
        layer?.let { current ->
            try {
                SurfaceControl.Transaction().use { it.reparent(current, null).apply() }
            } finally {
                current.release()
                layer = null
            }
        }
        while (available.isNotEmpty()) available.removeFirst().close()
    }

    private enum class Format(val hdrType: Int, val dataSpace: Int) {
        PQ(Display.HdrCapabilities.HDR_TYPE_HDR10, DataSpace.DATASPACE_BT2020_PQ),
        HLG(Display.HdrCapabilities.HDR_TYPE_HLG, DataSpace.DATASPACE_BT2020_HLG),
    }

    companion object {
        const val FALLBACK = -2
        private const val TAG = "PhiTKC-HDR"
        private val mainHandler = Handler(Looper.getMainLooper())
        private val releaseExecutor = Executors.newFixedThreadPool(3)
        private const val USAGE = HardwareBuffer.USAGE_CPU_WRITE_OFTEN or
            HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE or HardwareBuffer.USAGE_COMPOSER_OVERLAY

        private fun supportsDisplay(display: Display?, format: Format): Boolean =
            display?.hdrCapabilities?.supportedHdrTypes?.contains(format.hdrType) == true

        fun create(activity: Activity, session: Long, width: Int, height: Int, colorSpace: String): HdrPreviewSurface? {
            val format = when (colorSpace) {
                "rec2100_pq", "rec2100-pq" -> Format.PQ
                "rec2100_hlg", "rec2100-hlg" -> Format.HLG
                else -> return null
            }
            val supported = Build.VERSION.SDK_INT >= 33 &&
                supportsDisplay(activity.display, format) &&
                HardwareBuffer.isSupported(width, height, HardwareBuffer.RGBA_1010102, 1, USAGE)
            if (!supported) {
                Log.i(TAG, "${Build.MANUFACTURER} ${Build.MODEL}: ${format.name} output unavailable; using SDR")
                return null
            }
            return HdrPreviewSurface(activity, session, width, height, format)
        }
    }
}
