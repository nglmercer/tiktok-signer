package com.example.ttlsigner

import android.os.Bundle
import android.text.Editable
import android.text.TextWatcher
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.TextView
import androidx.fragment.app.Fragment
import androidx.fragment.app.activityViewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.events.LiveEvent
import com.example.ttlsigner.ui.CollapsibleCard
import com.example.ttlsigner.ui.StatusPillView
import com.google.android.material.chip.Chip
import com.google.android.material.chip.ChipGroup
import com.google.android.material.textfield.TextInputEditText
import kotlinx.coroutines.launch

/**
 * Events tab: the live reader. Filter chips per category, a search box, a
 * pause switch (stream keeps feeding points/actions while paused), per-type
 * counts, and the scrolling event tail.
 */
class EventsFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()
    private val chips = mutableMapOf<LiveEvent.Category, Chip>()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_events, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val statusPill: StatusPillView = view.findViewById(R.id.statusPill)
        val countsText: TextView = view.findViewById(R.id.countsText)
        val chipGroup: ChipGroup = view.findViewById(R.id.filterChips)
        val searchInput: TextInputEditText = view.findViewById(R.id.searchInput)
        val pauseButton: Button = view.findViewById(R.id.pauseButton)
        val clearButton: Button = view.findViewById(R.id.clearButton)
        val eventsCard: CollapsibleCard = view.findViewById(R.id.eventsCard)
        val eventsEmpty: TextView = view.findViewById(R.id.eventsEmpty)
        val eventList: RecyclerView = view.findViewById(R.id.eventList)
        val awardText: TextView = view.findViewById(R.id.awardText)

        eventList.layoutManager = LinearLayoutManager(requireContext())
        val adapter = EventAdapter()
        eventList.adapter = adapter

        // One checkable chip per category, built once; checked state follows
        // the view model's filter below.
        for (category in LiveEvent.Category.values()) {
            val chip = Chip(requireContext()).apply {
                text = category.name.lowercase()
                isCheckable = true
                isChecked = true
                setOnClickListener { vm.toggleCategory(category) }
            }
            chips[category] = chip
            chipGroup.addView(chip)
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
                        for ((category, chip) in chips) {
                            if (chip.isChecked != (category in filter.enabled)) {
                                chip.isChecked = category in filter.enabled
                            }
                        }
                        if (searchInput.text.toString() != filter.query) {
                            searchInput.setText(filter.query)
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
                        eventsCard.setTitle("$eventsLabel ($total)")
                    }
                }
                launch {
                    vm.counts.collect { counts ->
                        countsText.text = if (counts.isEmpty()) {
                            getString(R.string.events_empty_hint)
                        } else {
                            counts.entries.sortedBy { it.key.ordinal }.joinToString(" · ") {
                                "${it.key.name.lowercase()} ${it.value}"
                            }
                        }
                    }
                }
                launch {
                    vm.paused.collect { paused ->
                        pauseButton.text =
                            if (paused) getString(R.string.resume) else getString(R.string.pause)
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
}
