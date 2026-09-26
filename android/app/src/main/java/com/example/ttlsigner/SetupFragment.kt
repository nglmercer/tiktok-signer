package com.example.ttlsigner

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
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
import com.google.android.material.switchmaterial.SwitchMaterial
import com.google.android.material.textfield.TextInputEditText
import kotlinx.coroutines.launch

/**
 * Setup tab: everything needed to get live and stay live — connection, the
 * live feed, resolve + sign, the TTS toggles, signer/session maintenance, the
 * debug console, and about. The studio tabs (Events, Points, Actions) only
 * render; this tab connects.
 */
class SetupFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_setup, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val statusPill: StatusPillView = view.findViewById(R.id.statusPill)
        val roomInput: TextInputEditText = view.findViewById(R.id.roomInput)
        val connectButton: Button = view.findViewById(R.id.connectButton)
        val disconnectButton: Button = view.findViewById(R.id.disconnectButton)
        val setupProgress: LinearProgressIndicator = view.findViewById(R.id.setupProgress)
        val feedCard: CollapsibleCard = view.findViewById(R.id.feedCard)
        val feedButton: Button = view.findViewById(R.id.feedButton)
        val randomButton: Button = view.findViewById(R.id.randomButton)
        val feedError: TextView = view.findViewById(R.id.feedError)
        val feedList: RecyclerView = view.findViewById(R.id.feedList)
        val handleInput: TextInputEditText = view.findViewById(R.id.handleInput)
        val resolveButton: Button = view.findViewById(R.id.resolveButton)
        val signButton: Button = view.findViewById(R.id.signButton)
        val resultPlaceholder: TextView = view.findViewById(R.id.resultPlaceholder)
        val resultRoom: TextView = view.findViewById(R.id.resultRoom)
        val resultUser: TextView = view.findViewById(R.id.resultUser)
        val resultSummary: TextView = view.findViewById(R.id.resultSummary)
        val resultLatency: TextView = view.findViewById(R.id.resultLatency)
        val resultError: TextView = view.findViewById(R.id.resultError)
        val resultFields = listOf(resultRoom, resultUser, resultSummary, resultLatency)
        val ttsSwitch: SwitchMaterial = view.findViewById(R.id.ttsSwitch)
        val ttsJoinsSwitch: SwitchMaterial = view.findViewById(R.id.ttsJoinsSwitch)
        val nativeVersionValue: TextView = view.findViewById(R.id.nativeVersionValue)
        val bundleCacheValue: TextView = view.findViewById(R.id.bundleCacheValue)
        val redownloadButton: Button = view.findViewById(R.id.redownloadButton)
        val clearCacheButton: Button = view.findViewById(R.id.clearCacheButton)
        val guestValue: TextView = view.findViewById(R.id.guestValue)
        val resetSessionButton: Button = view.findViewById(R.id.resetSessionButton)
        val aboutValue: TextView = view.findViewById(R.id.aboutValue)

        feedList.layoutManager = LinearLayoutManager(requireContext())
        feedList.isNestedScrollingEnabled = false
        val feedAdapter = FeedAdapter { room -> vm.selectRoom(room) }
        feedList.adapter = feedAdapter

        connectButton.setOnClickListener { vm.connect(roomInput.text.toString()) }
        disconnectButton.setOnClickListener { vm.disconnect("user request") }
        feedButton.setOnClickListener { vm.refreshFeed() }
        randomButton.setOnClickListener { vm.connectRandom() }
        resolveButton.setOnClickListener { vm.resolve(handleInput.text.toString()) }
        signButton.setOnClickListener { vm.sign(handleInput.text.toString()) }

        ttsSwitch.isChecked = vm.ttsEnabled.value
        ttsSwitch.setOnCheckedChangeListener { _, checked -> vm.setTtsEnabled(checked) }
        ttsJoinsSwitch.isChecked = vm.speakJoins()
        ttsJoinsSwitch.setOnCheckedChangeListener { _, checked -> vm.setSpeakJoins(checked) }

        fun refreshGuest() {
            val names = vm.guestCookieNames()
            guestValue.text = names.ifEmpty {
                listOf(getString(R.string.settings_no_cookies))
            }.joinToString(", ")
        }

        fun refreshBundle(state: SessionViewModel.BundleUiState) {
            val cache = state.cache
            bundleCacheValue.text = cache?.let {
                "${it.sizeBytes / 1024} KB · ${age(it.ageMs)} · " +
                    if (it.shaOk) "digest OK" else "DIGEST MISMATCH"
            } ?: getString(R.string.settings_bundle_none)
        }

        viewLifecycleOwner.lifecycleScope.launch {
            nativeVersionValue.text = vm.nativeVersion()
            refreshBundle(vm.bundleState())
            refreshGuest()
            aboutValue.text =
                "${getString(R.string.settings_about_text)}\n\nApp ${appVersion()}"
        }

        fun setBundleBusy(busy: Boolean) {
            redownloadButton.isEnabled = !busy
            clearCacheButton.isEnabled = !busy
        }

        redownloadButton.setOnClickListener {
            viewLifecycleOwner.lifecycleScope.launch {
                setBundleBusy(true)
                try {
                    val chars = vm.redownloadBundle()
                    refreshBundle(vm.bundleState())
                    toast("bundle redownloaded ($chars chars)")
                } catch (e: Exception) {
                    toast("redownload failed: ${e.message}")
                } finally {
                    setBundleBusy(false)
                }
            }
        }
        clearCacheButton.setOnClickListener {
            viewLifecycleOwner.lifecycleScope.launch {
                setBundleBusy(true)
                try {
                    toast(vm.clearCaches())
                    refreshBundle(vm.bundleState())
                } catch (e: Exception) {
                    toast("clear failed: ${e.message}")
                } finally {
                    setBundleBusy(false)
                }
            }
        }
        resetSessionButton.setOnClickListener {
            vm.resetGuest()
            refreshGuest()
            toast("guest session reset")
        }

        val feedLabel = getString(R.string.feed_label)
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
                        setupProgress.visibility = if (busy) View.VISIBLE else View.GONE
                        feedButton.isEnabled = !busy
                        randomButton.isEnabled = !busy
                        resolveButton.isEnabled = !busy
                        signButton.isEnabled = !busy
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
                    // A feed tap suggests its handle — unless the user is typing.
                    vm.lastHandle.collect { handle ->
                        if (handle.isNotEmpty() && !handleInput.hasFocus() &&
                            handleInput.text.toString() != handle
                        ) {
                            handleInput.setText(handle)
                        }
                    }
                }
                launch {
                    vm.resolveInfo.collect { info ->
                        if (info == null) return@collect
                        resultPlaceholder.visibility = View.GONE
                        resultError.visibility = View.GONE
                        resultRoom.visibility = View.VISIBLE
                        resultUser.visibility = View.VISIBLE
                        resultRoom.text = "room ${info.roomId} · ${if (info.live) "LIVE" else "not live"}"
                        resultUser.text = "@${info.handle} (${info.nickname}) · ${info.title}"
                        resultLatency.visibility = View.VISIBLE
                        resultLatency.text = "resolve ${info.ms}ms"
                    }
                }
                launch {
                    vm.signResult.collect { result ->
                        if (result == null) return@collect
                        resultPlaceholder.visibility = View.GONE
                        resultError.visibility = View.GONE
                        resultRoom.visibility = View.VISIBLE
                        resultRoom.text = "room ${result.room}"
                        // A numeric sign has no user line; a resolve+sign keeps it.
                        val resolved = vm.resolveInfo.value
                        val showUser = resolved != null && resolved.roomId == result.room
                        resultUser.visibility = if (showUser) View.VISIBLE else View.GONE
                        resultSummary.visibility = View.VISIBLE
                        resultSummary.text = result.summary
                        resultLatency.visibility = View.VISIBLE
                        resultLatency.text = "sign ${result.ms}ms"
                    }
                }
                launch {
                    vm.taskError.collect { error ->
                        resultError.visibility = if (error == null) View.GONE else View.VISIBLE
                        resultError.text = error.orEmpty()
                        if (error != null) {
                            resultPlaceholder.visibility = View.GONE
                            resultFields.forEach { it.visibility = View.GONE }
                        }
                    }
                }
                launch {
                    // Keep the switch honest when TTS is toggled elsewhere.
                    vm.ttsEnabled.collect { enabled ->
                        if (ttsSwitch.isChecked != enabled) ttsSwitch.isChecked = enabled
                    }
                }
            }
        }
    }

    private fun age(ageMs: Long): String {
        val minutes = ageMs / 60_000
        if (minutes < 1) return "just now"
        if (minutes < 60) return "$minutes min old"
        val hours = minutes / 60
        if (hours < 24) return "$hours h old"
        return "${hours / 24} d old"
    }

    private fun appVersion(): String = runCatching {
        @Suppress("DEPRECATION")
        requireContext().packageManager.getPackageInfo(requireContext().packageName, 0).versionName
    }.getOrNull() ?: "?"

    private fun toast(msg: String) {
        Toast.makeText(requireContext(), msg, Toast.LENGTH_SHORT).show()
    }
}
