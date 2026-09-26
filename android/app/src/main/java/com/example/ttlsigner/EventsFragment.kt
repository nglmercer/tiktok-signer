package com.example.ttlsigner

import android.content.Context
import android.os.Bundle
import android.text.Editable
import android.text.TextWatcher
import android.util.TypedValue
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.widget.CheckBox
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.fragment.app.Fragment
import androidx.fragment.app.activityViewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.events.EventDisplayConfig
import com.example.ttlsigner.events.EventFilter
import com.example.ttlsigner.events.EventIcons
import com.example.ttlsigner.events.LiveEvent
import com.example.ttlsigner.ui.CollapsibleCard
import com.example.ttlsigner.ui.EmptyStateView
import com.example.ttlsigner.ui.StatusPillView
import com.example.ttlsigner.ui.UiKit
import com.example.ttlsigner.ui.UiKit.addMinimalDividers
import com.example.ttlsigner.ui.UiKit.dp
import com.google.android.material.button.MaterialButton
import com.google.android.material.textfield.TextInputEditText
import kotlinx.coroutines.launch

/**
 * Events tab, minimalist remake: the live reader behind an icon toolbar.
 * Search is one icon that expands a field on tap; the filter is a dropdown
 * panel of checkbox rows (icon + label + live count) with Select all / Clear;
 * the tune icon opens the row-style options. Rows are customizable
 * ([EventDisplayConfig]) and minimalist by default. Pause still only freezes
 * the reader — the stream keeps feeding points, actions, and speech.
 */
class EventsFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()
    private val checks = mutableMapOf<LiveEvent.Category, CheckBox>()
    private var lastCounts: Map<LiveEvent.Category, Int> = emptyMap()
    private var display = EventDisplayConfig()

    /**
     * Sync checkbox rows with the filter: checked state follows [filter], and
     * each row shows its live count (`chat · 12`), collapsing to the bare
     * label while at zero.
     */
    private fun renderChecks(filter: EventFilter, counts: Map<LiveEvent.Category, Int>) {
        for ((category, box) in checks) {
            val selected = category in filter.enabled
            if (box.isChecked != selected) box.isChecked = selected
            val label = category.name.lowercase()
            val count = counts[category] ?: 0
            val text = if (count > 0) "$label · $count" else label
            if (box.text.toString() != text) box.text = text
        }
    }

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_events, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val statusPill: StatusPillView = view.findViewById(R.id.statusPill)
        val pauseButton: MaterialButton = view.findViewById(R.id.pauseButton)
        val clearButton: MaterialButton = view.findViewById(R.id.clearButton)
        val searchButton: MaterialButton = view.findViewById(R.id.searchButton)
        val filterButton: MaterialButton = view.findViewById(R.id.filterButton)
        val displayButton: MaterialButton = view.findViewById(R.id.displayButton)
        val countsText: TextView = view.findViewById(R.id.countsText)
        val searchBar: LinearLayout = view.findViewById(R.id.searchBar)
        val searchInput: TextInputEditText = view.findViewById(R.id.searchInput)
        val closeSearchButton: MaterialButton = view.findViewById(R.id.closeSearchButton)
        val filterPanel: LinearLayout = view.findViewById(R.id.filterPanel)
        val filterBox: LinearLayout = view.findViewById(R.id.filterBox)
        val eventsCard: CollapsibleCard = view.findViewById(R.id.eventsCard)
        val eventsEmpty: EmptyStateView = view.findViewById(R.id.eventsEmpty)
        val eventList: RecyclerView = view.findViewById(R.id.eventList)
        val awardText: TextView = view.findViewById(R.id.awardText)

        display = EventDisplayConfig.load(requireContext())
        eventList.layoutManager = LinearLayoutManager(requireContext())
        eventList.addMinimalDividers()
        val adapter = EventAdapter(display)
        eventList.adapter = adapter

        // One checkbox row per category, built once; checked state follows
        // the view model's filter below.
        val muted = resolveAttrColor(
            requireContext(),
            com.google.android.material.R.attr.colorOnSurfaceVariant,
        )
        checks.clear()
        for (category in LiveEvent.Category.values()) {
            val row = LinearLayout(requireContext()).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = android.view.Gravity.CENTER_VERTICAL
                layoutParams = LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                )
            }
            val icon = ImageView(requireContext()).apply {
                layoutParams = LinearLayout.LayoutParams(
                    requireContext().dp(20),
                    requireContext().dp(20),
                ).apply { marginEnd = requireContext().dp(8) }
                setImageResource(EventIcons.res(category))
                imageTintList = android.content.res.ColorStateList.valueOf(muted)
            }
            val box = CheckBox(requireContext()).apply {
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                text = category.name.lowercase()
                isChecked = true
                setTextAppearance(
                    com.google.android.material.R.style.TextAppearance_Material3_BodyMedium,
                )
                setOnCheckedChangeListener { _, checked ->
                    val enabled = vm.filter.value.enabled
                    val next = if (checked) enabled + category else enabled - category
                    vm.setFilter(vm.filter.value.copy(enabled = next))
                }
            }
            row.addView(icon)
            row.addView(box)
            checks[category] = box
            filterBox.addView(row)
        }
        // Sync rows with any filter restored before this view existed.
        renderChecks(vm.filter.value, lastCounts)

        view.findViewById<View>(R.id.selectAllButton).setOnClickListener {
            vm.setFilter(vm.filter.value.selectAll())
        }
        view.findViewById<View>(R.id.clearFilterButton).setOnClickListener {
            vm.setFilter(vm.filter.value.clearSelection())
        }

        searchButton.setOnClickListener {
            val show = searchBar.visibility != View.VISIBLE
            searchBar.visibility = if (show) View.VISIBLE else View.GONE
            if (show) {
                searchInput.requestFocus()
                showKeyboard(searchInput)
            } else {
                hideKeyboard(searchInput)
            }
        }
        closeSearchButton.setOnClickListener {
            searchBar.visibility = View.GONE
            hideKeyboard(searchInput)
            if (vm.filter.value.query.isNotEmpty()) {
                vm.setFilter(vm.filter.value.copy(query = ""))
            }
        }
        searchInput.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) = Unit
            override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) = Unit
            override fun afterTextChanged(s: Editable?) {
                val query = s.toString()
                if (vm.filter.value.query != query) {
                    vm.setFilter(vm.filter.value.copy(query = query))
                }
            }
        })
        filterButton.setOnClickListener {
            filterPanel.visibility =
                if (filterPanel.visibility == View.VISIBLE) View.GONE else View.VISIBLE
        }
        displayButton.setOnClickListener { openDisplayOptions(adapter) }
        pauseButton.setOnClickListener { vm.setPaused(!vm.paused.value) }
        clearButton.setOnClickListener { vm.clearEvents() }

        val eventsLabel = getString(R.string.events_label)
        val scope = viewLifecycleOwner.lifecycleScope
        scope.launch {
            viewLifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch {
                    vm.live.collect { status -> statusPill.setStatus(status) }
                }
                launch {
                    vm.filter.collect { filter ->
                        renderChecks(filter, lastCounts)
                        if (searchInput.text.toString() != filter.query) {
                            searchInput.setText(filter.query)
                        }
                        // A query from anywhere (rotation, restore) reveals
                        // the field that holds it.
                        if (filter.query.isNotEmpty() && searchBar.visibility != View.VISIBLE) {
                            searchBar.visibility = View.VISIBLE
                        }
                    }
                }
                launch {
                    var lastSize = 0
                    vm.visibleEvents.collect { events ->
                        adapter.submitList(events) {
                            if (events.size > lastSize && events.isNotEmpty()) {
                                eventList.scrollToPosition(events.size - 1)
                            }
                            lastSize = events.size
                        }
                        eventsEmpty.visibility = if (events.isEmpty()) View.VISIBLE else View.GONE
                    }
                }
                launch {
                    vm.eventCount.collect { total ->
                        eventsCard.setTitleWithCount(eventsLabel, total)
                    }
                }
                launch {
                    vm.counts.collect { counts ->
                        lastCounts = counts
                        renderChecks(vm.filter.value, counts)
                        val pairs = counts.entries
                            .sortedBy { it.key.ordinal }
                            .map { it.key.name.lowercase() to it.value }
                        countsText.text =
                            UiKit.countsLine(pairs, getString(R.string.events_empty_hint))
                    }
                }
                launch {
                    vm.paused.collect { paused ->
                        pauseButton.setIconResource(
                            if (paused) R.drawable.ic_play else R.drawable.ic_pause,
                        )
                        pauseButton.contentDescription = getString(
                            if (paused) R.string.desc_resume else R.string.desc_pause,
                        )
                    }
                }
                launch {
                    vm.lastAward.collect { award ->
                        awardText.visibility = if (award == null) View.GONE else View.VISIBLE
                        if (award != null) {
                            awardText.text = "+${PointsAdapter.formatPoints(award.delta)} " +
                                "@${award.uniqueId} → ${PointsAdapter.formatPoints(award.total)}"
                        }
                    }
                }
            }
        }
    }

    /** Row-style options: four toggles, applied live and persisted. */
    private fun openDisplayOptions(adapter: EventAdapter) {
        val labels = listOf(
            getString(R.string.display_compact),
            getString(R.string.display_badge),
            getString(R.string.display_time),
            getString(R.string.display_single_line),
        ).toTypedArray()
        val checked = booleanArrayOf(
            display.compact,
            display.showBadge,
            display.showTime,
            display.singleLine,
        )
        AlertDialog.Builder(requireContext())
            .setTitle(R.string.display_title)
            .setMultiChoiceItems(labels, checked) { _, which, isChecked ->
                display = when (which) {
                    0 -> display.copy(compact = isChecked)
                    1 -> display.copy(showBadge = isChecked)
                    2 -> display.copy(showTime = isChecked)
                    else -> display.copy(singleLine = isChecked)
                }
                EventDisplayConfig.save(requireContext(), display)
                adapter.setDisplay(display)
            }
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }

    private fun showKeyboard(target: View) {
        val imm = requireContext().getSystemService(Context.INPUT_METHOD_SERVICE)
            as InputMethodManager
        imm.showSoftInput(target, InputMethodManager.SHOW_IMPLICIT)
    }

    private fun hideKeyboard(target: View) {
        val imm = requireContext().getSystemService(Context.INPUT_METHOD_SERVICE)
            as InputMethodManager
        imm.hideSoftInputFromWindow(target.windowToken, 0)
    }

    private fun resolveAttrColor(context: Context, attr: Int): Int {
        val out = TypedValue()
        context.theme.resolveAttribute(attr, out, true)
        return out.data
    }
}
