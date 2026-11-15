package com.cnmdbook.chartpicker

import android.app.Activity
import android.app.Dialog
import android.content.pm.ActivityInfo
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.util.TypedValue
import android.view.Choreographer
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.FrameLayout
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import java.nio.ByteBuffer
import java.util.Locale

// The library is already loaded by Tauri before any plugin command executes.
internal object PreviewBridge {
    external fun readFrame(session: Long, buffer: ByteBuffer): Int
    external fun touch(session: Long, contact: Long, phase: Int, x: Float, y: Float, time: Double)
    external fun active(session: Long, active: Int)
    external fun close(session: Long)
    external fun fps(session: Long): Float
}

internal class PreviewDialog(private val activity: Activity, private val session: Long,
                             width: Int, height: Int, showFps: Boolean,
                             private val completed: () -> Unit) : Dialog(activity, android.R.style.Theme_Black_NoTitleBar_Fullscreen) {
    private var finished = false
    private val previousOrientation = activity.requestedOrientation
    private val gameOrientation = if (width >= height) ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
                                  else ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT
    private var orientationApplied = false
    private val preview = PreviewView(activity, session, width, height, showFps) { dismiss() }

    init {
        // The game's pause menu owns normal exit; readFrame(-1) also closes a
        // completed or failed worker, including a failure before the first frame.
        setCancelable(false)
        setCanceledOnTouchOutside(false)
        val content = FrameLayout(activity)
        content.setBackgroundColor(Color.BLACK)
        content.addView(preview, FrameLayout.LayoutParams(-1, -1))
        setContentView(content)
        window?.setBackgroundDrawableResource(android.R.color.transparent)
        window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setOnDismissListener {
            if (!finished) {
                finished = true
                preview.stop()
                PreviewBridge.close(session)
                restoreOrientation()
                completed()
            }
        }
    }

    override fun show() {
        activity.requestedOrientation = gameOrientation
        orientationApplied = true
        try {
            super.show()
        } catch (error: Exception) {
            restoreOrientation()
            throw error
        }
    }

    private fun restoreOrientation() {
        if (orientationApplied) {
            orientationApplied = false
            if (!activity.isDestroyed) activity.requestedOrientation = previousOrientation
        }
    }

    private fun enterFullscreen() {
        val gameWindow = window ?: return
        WindowCompat.setDecorFitsSystemWindows(gameWindow, false)
        WindowInsetsControllerCompat(gameWindow, gameWindow.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    override fun onStart() {
        super.onStart()
        window?.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        enterFullscreen()
        PreviewBridge.active(session, 1)
        preview.start()
    }

    fun setActive(active: Boolean) {
        PreviewBridge.active(session, if (active) 1 else 0)
        if (active) {
            enterFullscreen()
            preview.start()
        } else preview.stop()
    }
}

private class PreviewView(activity: Activity, private val session: Long,
                          private val imageWidth: Int, private val imageHeight: Int,
                          private val showFps: Boolean,
                          private val finished: () -> Unit) : View(activity), Choreographer.FrameCallback {
    private val bitmap = Bitmap.createBitmap(imageWidth, imageHeight, Bitmap.Config.ARGB_8888)
    private val pixels = ByteBuffer.allocateDirect(imageWidth * imageHeight * 4)
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG)
    private val frame = RectF()
    private var running = false
    private var hasImage = false
    private var fpsText = "FPS: —"
    private var lastFpsUpdate = 0L
    private val fpsPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        textSize = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, 14f, resources.displayMetrics)
        textAlign = Paint.Align.RIGHT
    }
    private val fpsBackground = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0x99000000.toInt() }

    fun start() {
        if (!running) {
            running = true
            Choreographer.getInstance().postFrameCallback(this)
        }
    }
    fun stop() {
        running = false
        Choreographer.getInstance().removeFrameCallback(this)
    }
    override fun doFrame(frameTimeNanos: Long) {
        if (!running) return
        when (PreviewBridge.readFrame(session, pixels)) {
            -1 -> { stop(); finished(); return }
            1 -> {
                pixels.rewind()
                bitmap.copyPixelsFromBuffer(pixels)
                hasImage = true
                if (showFps && frameTimeNanos - lastFpsUpdate >= 500_000_000L) {
                    val fps = PreviewBridge.fps(session)
                    fpsText = if (fps.isFinite()) String.format(Locale.ROOT, "FPS: %.1f", fps.coerceAtLeast(0f)) else "FPS: —"
                    lastFpsUpdate = frameTimeNanos
                }
                invalidate()
            }
        }
        Choreographer.getInstance().postFrameCallback(this)
    }
    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        val scale = minOf(w.toFloat() / imageWidth, h.toFloat() / imageHeight)
        val rw = imageWidth * scale
        val rh = imageHeight * scale
        frame.set((w-rw)/2, (h-rh)/2, (w+rw)/2, (h+rh)/2)
    }
    override fun onDraw(canvas: Canvas) {
        canvas.drawColor(Color.BLACK)
        if (hasImage) canvas.drawBitmap(bitmap, null, frame, paint)
        else {
            paint.color = Color.WHITE
            paint.textSize = 18 * resources.displayMetrics.scaledDensity
            canvas.drawText("Loading…", 24f, height / 2f, paint)
        }
        if (showFps && hasImage) {
            val margin = 12 * resources.displayMetrics.density
            val padding = 6 * resources.displayMetrics.density
            val x = width - margin
            val y = height - margin - fpsPaint.fontMetrics.descent
            canvas.drawRoundRect(x - fpsPaint.measureText(fpsText) - padding,
                y + fpsPaint.fontMetrics.ascent - padding,
                x + padding, y + fpsPaint.fontMetrics.descent + padding,
                padding, padding, fpsBackground)
            canvas.drawText(fpsText, x, y, fpsPaint)
        }
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (frame.width() <= 0 || frame.height() <= 0) return true
        fun send(index: Int, phase: Int, sample: Int = -1) {
            val x = if (sample >= 0) event.getHistoricalX(index, sample) else event.getX(index)
            val y = if (sample >= 0) event.getHistoricalY(index, sample) else event.getY(index)
            val time = if (sample >= 0) event.getHistoricalEventTime(sample) else event.eventTime
            PreviewBridge.touch(session, event.getPointerId(index).toLong(), phase,
                (x-frame.left)/frame.width()*imageWidth,
                (y-frame.top)/frame.height()*imageHeight, time / 1000.0)
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> send(event.actionIndex, 0)
            MotionEvent.ACTION_MOVE -> {
                for (sample in 0 until event.historySize) for (index in 0 until event.pointerCount) send(index, 1, sample)
                for (index in 0 until event.pointerCount) send(index, 1)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> send(event.actionIndex, 2)
            MotionEvent.ACTION_CANCEL -> for (index in 0 until event.pointerCount) send(index, 3)
        }
        return true
    }
    override fun onDetachedFromWindow() {
        stop()
        bitmap.recycle()
        super.onDetachedFromWindow()
    }
}
