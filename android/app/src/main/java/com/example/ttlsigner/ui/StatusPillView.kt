package com.example.ttlsigner.ui

import android.content.Context
import android.graphics.drawable.GradientDrawable
import android.util.AttributeSet
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.content.ContextCompat
import com.example.ttlsigner.R
import com.example.ttlsigner.SessionViewModel

/**
 * A colored dot plus a one-line connection state. The Live tab header shows at
 * a glance whether the stream is down, opening, or pushing events.
 */
class StatusPillView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : LinearLayout(context, attrs, defStyleAttr) {

    private val dot: View
    private val label: TextView

    init {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        val density = resources.displayMetrics.density
        val dotSize = (8 * density).toInt()
        dot = View(context).apply {
            layoutParams = LayoutParams(dotSize, dotSize)
            background = GradientDrawable().apply { shape = GradientDrawable.OVAL }
        }
        label = TextView(context).apply {
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            ).apply { marginStart = (8 * density).toInt() }
            setTextAppearance(com.google.android.material.R.style.TextAppearance_Material3_BodySmall)
            maxLines = 1
            ellipsize = android.text.TextUtils.TruncateAt.END
        }
        addView(dot)
        addView(label)
        setStatus(SessionViewModel.LiveStatus.Idle)
    }

    fun setStatus(status: SessionViewModel.LiveStatus) {
        val color = when (status) {
            is SessionViewModel.LiveStatus.Idle -> R.color.status_idle
            is SessionViewModel.LiveStatus.Connecting -> R.color.status_connecting
            is SessionViewModel.LiveStatus.Live -> R.color.status_live
        }
        (dot.background as GradientDrawable).setColor(ContextCompat.getColor(context, color))
        label.text = when (status) {
            is SessionViewModel.LiveStatus.Idle -> "Disconnected"
            is SessionViewModel.LiveStatus.Connecting -> "Connecting to ${status.room}…"
            is SessionViewModel.LiveStatus.Live -> "Live · room ${status.room}"
        }
    }
}
