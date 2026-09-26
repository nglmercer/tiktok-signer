package com.example.ttlsigner.ui

import android.content.Context
import android.util.AttributeSet
import android.util.TypedValue
import android.view.View
import com.example.ttlsigner.R

/**
 * A 1dp hairline in the theme's outline-variant color, with optional
 * start/end insets. Divides flat minimalist sections where a card edge would
 * be visual noise:
 *
 * ```xml
 * <com.example.ttlsigner.ui.MinimalDivider
 *     android:layout_width="match_parent"
 *     android:layout_height="1dp" />
 * ```
 */
class MinimalDivider @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : View(context, attrs, defStyleAttr) {

    init {
        val out = TypedValue()
        context.theme.resolveAttribute(
            com.google.android.material.R.attr.colorOutlineVariant,
            out,
            true,
        )
        setBackgroundColor(out.data)

        context.obtainStyledAttributes(attrs, R.styleable.MinimalDivider).use { a ->
            val start = a.getDimensionPixelSize(R.styleable.MinimalDivider_dividerInsetStart, 0)
            val end = a.getDimensionPixelSize(R.styleable.MinimalDivider_dividerInsetEnd, 0)
            if (start != 0 || end != 0) setPadding(start, 0, end, 0)
        }
    }
}
