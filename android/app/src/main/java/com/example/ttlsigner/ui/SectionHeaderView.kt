package com.example.ttlsigner.ui

import android.content.Context
import android.util.AttributeSet
import android.view.Gravity
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import com.example.ttlsigner.R

/**
 * One minimalist section header: a small letterspaced label plus an optional
 * count suffix ("Rates", "Rates · 6"). Cards that don't collapse (rates,
 * manual adjust, TTS, signer) use this instead of hand-rolled title
 * `TextView`s, so every heading shares one style:
 *
 * ```xml
 * <com.example.ttlsigner.ui.SectionHeaderView
 *     android:layout_width="match_parent"
 *     android:layout_height="wrap_content"
 *     app:sectionTitle="@string/rates_title" />
 * ```
 */
class SectionHeaderView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : LinearLayout(context, attrs, defStyleAttr) {

    private val labelView: TextView
    private var label = ""

    init {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        labelView = TextView(context).apply {
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
            setTextAppearance(R.style.TextAppearance_TikTools_SectionLabel)
        }
        addView(labelView)

        context.obtainStyledAttributes(attrs, R.styleable.SectionHeaderView).use { a ->
            label = a.getString(R.styleable.SectionHeaderView_sectionTitle).orEmpty()
            val count = a.getString(R.styleable.SectionHeaderView_sectionCount)
            labelView.text = render(label, count)
        }
    }

    fun setTitle(title: CharSequence) {
        label = title.toString()
        labelView.text = render(label, null)
    }

    /** A null or blank count renders the bare label. */
    fun setCount(count: CharSequence?) {
        labelView.text = render(label, count?.toString())
    }

    private fun render(label: String, count: String?): String =
        if (count.isNullOrBlank()) label else "$label · $count"
}
