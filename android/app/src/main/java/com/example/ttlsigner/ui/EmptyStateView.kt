package com.example.ttlsigner.ui

import android.content.Context
import android.util.AttributeSet
import android.view.Gravity
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.content.ContextCompat
import com.example.ttlsigner.R
import com.example.ttlsigner.ui.UiKit.dp
import com.google.android.material.R as MaterialR

/**
 * The minimalist empty state: a small muted icon over one centered line.
 * Every list (events, leaderboard, actions) shows this instead of a bare
 * `TextView`, so "nothing here yet" looks the same everywhere:
 *
 * ```xml
 * <com.example.ttlsigner.ui.EmptyStateView
 *     android:id="@+id/eventsEmpty"
 *     android:layout_width="match_parent"
 *     android:layout_height="wrap_content"
 *     app:emptyIcon="@drawable/ic_ev_chat"
 *     app:emptyMessage="@string/events_empty" />
 * ```
 *
 * Fragments only toggle `visibility`; the message comes from XML or
 * [setMessage].
 */
class EmptyStateView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : LinearLayout(context, attrs, defStyleAttr) {

    private val messageView: TextView

    init {
        orientation = VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        val pad = context.dp(16)
        setPadding(pad, pad, pad, pad)

        val iconView = ImageView(context).apply {
            layoutParams = LayoutParams(context.dp(28), context.dp(28))
            imageTintList = ContextCompat.getColorStateList(
                context,
                com.google.android.material.R.color.material_on_surface_disabled,
            )
        }
        messageView = TextView(context).apply {
            layoutParams = LayoutParams(
                LayoutParams.WRAP_CONTENT,
                LayoutParams.WRAP_CONTENT,
            ).apply { topMargin = context.dp(8) }
            gravity = Gravity.CENTER
            setTextAppearance(MaterialR.style.TextAppearance_Material3_BodyMedium)
            // Muted tone from the theme, not a fixed color, so day/night and
            // dynamic color keep working.
            val out = android.util.TypedValue()
            context.theme.resolveAttribute(
                com.google.android.material.R.attr.colorOnSurfaceVariant,
                out,
                true,
            )
            setTextColor(out.data)
        }
        addView(iconView)
        addView(messageView)

        context.obtainStyledAttributes(attrs, R.styleable.EmptyStateView).use { a ->
            messageView.text = a.getString(R.styleable.EmptyStateView_emptyMessage).orEmpty()
            val icon = a.getResourceId(R.styleable.EmptyStateView_emptyIcon, 0)
            if (icon != 0) iconView.setImageResource(icon)
            else iconView.visibility = GONE
        }
    }

    fun setMessage(message: CharSequence) {
        messageView.text = message
    }
}
