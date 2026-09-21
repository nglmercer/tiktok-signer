package com.example.ttlsigner.ui

import android.content.Context
import android.util.AttributeSet
import android.util.TypedValue
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import com.example.ttlsigner.R
import com.google.android.material.card.MaterialCardView

/**
 * A card with a tappable header that expands/collapses its body. Children
 * declared inside this view in XML land in the collapsible body:
 *
 * ```xml
 * <com.example.ttlsigner.ui.CollapsibleCard
 *     android:layout_width="match_parent"
 *     android:layout_height="wrap_content"
 *     app:cardTitle="@string/feed_label">
 *     <!-- body views -->
 * </com.example.ttlsigner.ui.CollapsibleCard>
 * ```
 *
 * In a weighted `LinearLayout` slot the card gives its weight back while
 * collapsed, so siblings take the freed space.
 */
class CollapsibleCard @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : MaterialCardView(context, attrs, defStyleAttr) {

    private val titleView: TextView
    private val chevron: TextView
    private val body: FrameLayout
    private var expanded = true
    private var savedWeight = 0f
    private var redirectChildren = false

    init {
        val pad = (12 * resources.displayMetrics.density).toInt()
        val ripple = TypedValue()
        context.theme.resolveAttribute(android.R.attr.selectableItemBackground, ripple, true)
        val header = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = android.view.Gravity.CENTER_VERTICAL
            setPadding(pad, pad, pad, pad)
            setBackgroundResource(ripple.resourceId)
            isClickable = true
            isFocusable = true
        }
        titleView = TextView(context).apply {
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
            setTextAppearance(com.google.android.material.R.style.TextAppearance_Material3_TitleSmall)
        }
        chevron = TextView(context).apply {
            setTextAppearance(com.google.android.material.R.style.TextAppearance_Material3_TitleSmall)
        }
        header.addView(titleView)
        header.addView(chevron)
        // A vertical container: the card itself is a frame, so header and body
        // would otherwise stack on top of each other.
        body = FrameLayout(context).apply {
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            )
        }
        val container = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            )
        }
        container.addView(header)
        container.addView(body)
        super.addView(container)
        header.setOnClickListener { setExpanded(!expanded) }

        context.obtainStyledAttributes(attrs, R.styleable.CollapsibleCard).use { a ->
            titleView.text = a.getString(R.styleable.CollapsibleCard_cardTitle).orEmpty()
            expanded = a.getBoolean(R.styleable.CollapsibleCard_startExpanded, true)
        }
        redirectChildren = true
        applyState()
    }

    override fun addView(child: View, index: Int, params: ViewGroup.LayoutParams) {
        if (redirectChildren) body.addView(child, params)
        else super.addView(child, index, params)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        // Layout params only exist once attached; a card that starts collapsed
        // in a weight slot must release the weight here.
        applyState()
    }

    fun setTitle(title: CharSequence) {
        titleView.text = title
    }

    fun setExpanded(value: Boolean) {
        if (value == expanded) return
        expanded = value
        applyState()
    }

    private fun applyState() {
        body.visibility = if (expanded) View.VISIBLE else View.GONE
        chevron.setText(if (expanded) R.string.expanded else R.string.collapsed)
        val lp = layoutParams as? LinearLayout.LayoutParams ?: return
        if (!expanded && lp.height == 0 && lp.weight > 0f) {
            savedWeight = lp.weight
            lp.height = ViewGroup.LayoutParams.WRAP_CONTENT
            lp.weight = 0f
            layoutParams = lp
        } else if (expanded && savedWeight > 0f) {
            lp.height = 0
            lp.weight = savedWeight
            layoutParams = lp
        }
    }
}
