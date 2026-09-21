package com.example.ttlsigner

import android.os.Bundle
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
import com.example.ttlsigner.ui.CollapsibleCard
import com.example.ttlsigner.ui.StatusPillView
import com.google.android.material.progressindicator.LinearProgressIndicator
import com.google.android.material.textfield.TextInputEditText
import kotlinx.coroutines.launch

/** Live tab: connection status, the live feed, and the event tail. */
class LiveFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_live, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val statusPill: StatusPillView = view.findViewById(R.id.statusPill)
        val roomInput: TextInputEditText = view.findViewById(R.id.roomInput)
        val connectButton: Button = view.findViewById(R.id.connectButton)
        val disconnectButton: Button = view.findViewById(R.id.disconnectButton)
        val liveProgress: LinearProgressIndicator = view.findViewById(R.id.liveProgress)
        val feedCard: CollapsibleCard = view.findViewById(R.id.feedCard)
        val feedButton: Button = view.findViewById(R.id.feedButton)
        val randomButton: Button = view.findViewById(R.id.randomButton)
        val feedError: TextView = view.findViewById(R.id.feedError)
        val eventsCard: CollapsibleCard = view.findViewById(R.id.eventsCard)
        val eventsEmpty: TextView = view.findViewById(R.id.eventsEmpty)

        val feedList: RecyclerView = view.findViewById(R.id.feedList)
        feedList.layoutManager = LinearLayoutManager(requireContext())
        val feedAdapter = FeedAdapter { room -> vm.selectRoom(room) }
        feedList.adapter = feedAdapter

        val eventList: RecyclerView = view.findViewById(R.id.eventList)
        eventList.layoutManager = LinearLayoutManager(requireContext())
        val eventAdapter = EventAdapter()
        eventList.adapter = eventAdapter

        connectButton.setOnClickListener { vm.connect(roomInput.text.toString()) }
        disconnectButton.setOnClickListener { vm.disconnect("user request") }
        feedButton.setOnClickListener { vm.refreshFeed() }
        randomButton.setOnClickListener { vm.connectRandom() }
        view.findViewById<View>(R.id.eventsClearButton).setOnClickListener { vm.clearEvents() }

        val feedLabel = getString(R.string.feed_label)
        val eventsLabel = getString(R.string.events_label)
        val scope = viewLifecycleOwner.lifecycleScope
        scope.launch {
            viewLifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch {
                    vm.live.collect { status ->
                        statusPill.setStatus(status)
                        connectButton.isEnabled = status is SessionViewModel.LiveStatus.Idle
                        disconnectButton.isEnabled = status !is SessionViewModel.LiveStatus.Idle
                    }
                }
                launch {
                    vm.busy.collect { busy ->
                        liveProgress.visibility = if (busy) View.VISIBLE else View.GONE
                        feedButton.isEnabled = !busy
                        randomButton.isEnabled = !busy
                    }
                }
                launch {
                    vm.feed.collect { state ->
                        feedAdapter.submitList(state.rooms)
                        feedCard.setTitle("$feedLabel (${state.rooms.size})")
                        feedError.visibility = if (state.error == null) View.GONE else View.VISIBLE
                        feedError.text = state.error.orEmpty()
                    }
                }
                launch {
                    vm.events.collect { lines ->
                        eventAdapter.submitList(lines)
                        eventsEmpty.visibility = if (lines.isEmpty()) View.VISIBLE else View.GONE
                        if (lines.isNotEmpty()) eventList.scrollToPosition(lines.size - 1)
                    }
                }
                launch {
                    vm.eventCount.collect { count ->
                        eventsCard.setTitle("$eventsLabel ($count)")
                    }
                }
            }
        }
    }
}
