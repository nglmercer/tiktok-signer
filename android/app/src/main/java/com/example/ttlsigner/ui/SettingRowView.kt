package com.example.ttlsigner.ui

import android.content.Context
import android.util.AttributeSet
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import com.example.ttlsigner.R
import com.example.ttlsigner.ui.UiKit.dp

/**
 * A minimalist label/value row for the Setup maintenance section: a small
 * muted label over a slot. Children declared inside this view in XML land in
 * the value slot, so existing value IDs keep resolving:
 *
 * ```xml
 * <com.example.ttlsigner.ui.SettingRowView
 *     android:layout_width="match_parent"
 *     android:layout_height="wrap_content"
 *     app:rowLabel="Native core">
 *     <TextView
 *         android:id="@+id/nativeVersionValue"
 *         android:layout_width="match_parent"
 *         android:layout_height="wrap_content" />
 * </com.example.ttlsigner.ui.SettingRowView>
 * ```
 */
class SettingRowView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : LinearLayout(context, attrs, defStyleAttr) {

    private val labelView: TextView
    private val slot: FrameLayout
    private var redirectChildren = false

    init {
        orientation = VERTICAL
        val gap = context.dp(2)
        labelView = TextView(context).apply {
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            ).apply { bottomMargin = gap }
            setTextAppearance(R.style.TextAppearance_TikTools_SectionLabel)
        }
        slot = FrameLayout(context).apply {
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
        }
        super.addView(labelView)
        super.addView(slot)

        context.obtainStyledAttributes(attrs, R.styleable.SettingRowView).use { a ->
            labelView.text = a.getString(R.styleable.SettingRowView_rowLabel).orEmpty()
        }
        redirectChildren = true
    }

    override fun addView(child: View, index: Int, params: ViewGroup.LayoutParams) {
        if (redirectChildren) slot.addView(child, params)
        else super.addView(child, index, params)
    }

    fun setLabel(label: CharSequence) {
        labelView.text = label
    }
}
