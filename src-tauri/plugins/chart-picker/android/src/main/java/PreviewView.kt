package com.cnmdbook.chartpicker

import android.app.Activity
import android.app.Dialog
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.os.SystemClock
import android.view.Choreographer
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.TextView
import java.nio.ByteBuffer

// The library is already loaded by Tauri before any plugin command executes.
internal object PreviewBridge {
    external fun readFrame(session: Long, buffer: ByteBuffer): Int
    external fun touch(session: Long, contact: Long, phase: Int, x: Float, y: Float, time: Double)
    external fun active(session: Long, active: Int)
    external fun close(session: Long)
}

internal class PreviewDialog(activity: Activity, private val session: Long,
                             width: Int, height: Int, private val completed: () -> Unit) : Dialog(activity) {
    private var finished = false
    private val preview = PreviewView(activity, session, width, height) { dismiss() }

    init {
        val content = FrameLayout(activity)
        content.setBackgroundColor(Color.BLACK)
        content.addView(preview, FrameLayout.LayoutParams(-1, -1))
        val close = TextView(activity).apply {
            text = "×"
            textSize = 30f
            setTextColor(Color.WHITE)
            setBackgroundColor(0x88000000.toInt())
            gravity = android.view.Gravity.CENTER
            contentDescription = "Close preview"
            setOnClickListener { dismiss() }
        }
        val size = (48 * activity.resources.displayMetrics.density).toInt()
        content.addView(close, FrameLayout.LayoutParams(size, size, android.view.Gravity.TOP or android.view.Gravity.END))
        setContentView(content)
        window?.setBackgroundDrawableResource(android.R.color.transparent)
        window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setOnDismissListener {
            if (!finished) {
                finished = true
                preview.stop()
                PreviewBridge.close(session)
                completed()
            }
        }
    }

    override fun onStart() {
        super.onStart()
        window?.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        PreviewBridge.active(session, 1)
        preview.start()
    }

    fun setActive(active: Boolean) {
        PreviewBridge.active(session, if (active) 1 else 0)
        if (active) preview.start() else preview.stop()
    }
}

private class PreviewView(activity: Activity, private val session: Long,
                          private val imageWidth: Int, private val imageHeight: Int,
                          private val finished: () -> Unit) : View(activity), Choreographer.FrameCallback {
    private val bitmap = Bitmap.createBitmap(imageWidth, imageHeight, Bitmap.Config.ARGB_8888)
    private val pixels = ByteBuffer.allocateDirect(imageWidth * imageHeight * 4)
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG)
    private val frame = RectF()
    private var running = false
    private var hasImage = false

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
