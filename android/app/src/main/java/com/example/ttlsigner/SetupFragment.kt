package com.example.ttlsigner

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.RadioGroup
import android.widget.TextView
import android.widget.Toast
import androidx.fragment.app.Fragment
import androidx.fragment.app.activityViewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.tts.TtsEngine
import com.example.ttlsigner.tts.supertonic.SupertonicSpeaker
import com.example.ttlsigner.ui.CollapsibleCard
import com.example.ttlsigner.ui.StatusPillView
import com.example.ttlsigner.ui.UiKit.addMinimalDividers
import com.google.android.material.progressindicator.LinearProgressIndicator
import com.google.android.material.switchmaterial.SwitchMaterial
import com.google.android.material.textfield.TextInputEditText
import kotlinx.coroutines.launch

/**
 * Setup tab: direct login and nothing else in the way — a username input plus
 * the live feed. Typing a handle (or numeric room id) and tapping Connect
 * resolves, signs, and opens the stream in one step; tapping a feed room does
 * the same. Below the login: TTS toggles, signer/session maintenance, the
 * debug console, and about.
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
        val usernameInput: TextInputEditText = view.findViewById(R.id.usernameInput)
        val connectButton: Button = view.findViewById(R.id.connectButton)
        val disconnectButton: Button = view.findViewById(R.id.disconnectButton)
        val setupProgress: LinearProgressIndicator = view.findViewById(R.id.setupProgress)
        val feedCard: CollapsibleCard = view.findViewById(R.id.feedCard)
        val feedButton: Button = view.findViewById(R.id.feedButton)
        val randomButton: Button = view.findViewById(R.id.randomButton)
        val feedError: TextView = view.findViewById(R.id.feedError)
        val feedList: RecyclerView = view.findViewById(R.id.feedList)
        val ttsEngineGroup: RadioGroup = view.findViewById(R.id.ttsEngineGroup)
        val ttsJoinsSwitch: SwitchMaterial = view.findViewById(R.id.ttsJoinsSwitch)
        val supertonicStatus: TextView = view.findViewById(R.id.supertonicStatus)
        val supertonicProgress: LinearProgressIndicator = view.findViewById(R.id.supertonicProgress)
        val downloadSupertonicButton: Button = view.findViewById(R.id.downloadSupertonicButton)
        val testSpeechButton: Button = view.findViewById(R.id.testSpeechButton)
        val nativeVersionValue: TextView = view.findViewById(R.id.nativeVersionValue)
        val bundleCacheValue: TextView = view.findViewById(R.id.bundleCacheValue)
        val redownloadButton: Button = view.findViewById(R.id.redownloadButton)
        val clearCacheButton: Button = view.findViewById(R.id.clearCacheButton)
        val guestValue: TextView = view.findViewById(R.id.guestValue)
        val resetSessionButton: Button = view.findViewById(R.id.resetSessionButton)
        val aboutValue: TextView = view.findViewById(R.id.aboutValue)

        feedList.layoutManager = LinearLayoutManager(requireContext())
        feedList.addMinimalDividers()
        feedList.isNestedScrollingEnabled = false
        val feedAdapter = FeedAdapter { room -> vm.selectRoom(room) }
        feedList.adapter = feedAdapter

        connectButton.setOnClickListener { vm.connect(usernameInput.text.toString()) }
        disconnectButton.setOnClickListener { vm.disconnect("user request") }
        feedButton.setOnClickListener { vm.refreshFeed() }
        randomButton.setOnClickListener { vm.connectRandom() }

        fun checkEngine(engine: TtsEngine) {
            val id = when (engine) {
                TtsEngine.OFF -> R.id.ttsEngineOff
                TtsEngine.DEVICE -> R.id.ttsEngineDevice
                TtsEngine.SUPERTONIC -> R.id.ttsEngineSuper
            }
            if (ttsEngineGroup.checkedRadioButtonId != id) ttsEngineGroup.check(id)
        }
        checkEngine(vm.ttsEngine.value)
        ttsEngineGroup.setOnCheckedChangeListener { _, id ->
            vm.setTtsEngine(
                when (id) {
                    R.id.ttsEngineDevice -> TtsEngine.DEVICE
                    R.id.ttsEngineSuper -> TtsEngine.SUPERTONIC
                    else -> TtsEngine.OFF
                },
            )
        }
        ttsJoinsSwitch.isChecked = vm.speakJoins()
        ttsJoinsSwitch.setOnCheckedChangeListener { _, checked -> vm.setSpeakJoins(checked) }
        downloadSupertonicButton.setOnClickListener { vm.downloadSupertonicModels() }
        testSpeechButton.setOnClickListener { vm.testSpeech() }

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
                        connectButton.isEnabled = !busy &&
                            vm.live.value is SessionViewModel.LiveStatus.Idle
                        feedButton.isEnabled = !busy
                        randomButton.isEnabled = !busy
                    }
                }
                launch {
                    vm.feed.collect { state ->
                        feedAdapter.submitList(state.rooms)
                        feedCard.setTitleWithCount(feedLabel, state.rooms.size)
                        feedError.visibility = if (state.error == null) View.GONE else View.VISIBLE
                        feedError.text = state.error.orEmpty()
                    }
                }
                launch {
                    // A feed tap suggests its handle — unless the user is typing.
                    vm.lastHandle.collect { handle ->
                        if (handle.isNotEmpty() && !usernameInput.hasFocus() &&
                            usernameInput.text.toString() != handle
                        ) {
                            usernameInput.setText(handle)
                        }
                    }
                }
                launch {
                    vm.ttsEngine.collect { checkEngine(it) }
                }
                launch {
                    vm.supertonicModels.collect { models ->
                        supertonicProgress.visibility =
                            if (models.downloading) View.VISIBLE else View.GONE
                        models.progress?.let { supertonicProgress.setProgressCompat((it * 100).toInt(), true) }
                        downloadSupertonicButton.isEnabled = !models.downloading && !models.ready
                        val engineLine = when (val state = vm.supertonicState.value) {
                            is SupertonicSpeaker.State.Idle -> ""
                            is SupertonicSpeaker.State.Loading -> " · engine loading…"
                            is SupertonicSpeaker.State.Ready -> " · engine ready"
                            is SupertonicSpeaker.State.Speaking -> " · speaking…"
                            is SupertonicSpeaker.State.Error -> " · ${state.detail}"
                        }
                        supertonicStatus.text = "SuperTonic 3: ${models.detail}$engineLine"
                    }
                }
                launch {
                    // Re-render the status line when the engine state changes.
                    vm.supertonicState.collect {
                        val models = vm.supertonicModels.value
                        val engineLine = when (val state = it) {
                            is SupertonicSpeaker.State.Idle -> ""
                            is SupertonicSpeaker.State.Loading -> " · engine loading…"
                            is SupertonicSpeaker.State.Ready -> " · engine ready"
                            is SupertonicSpeaker.State.Speaking -> " · speaking…"
                            is SupertonicSpeaker.State.Error -> " · ${state.detail}"
                        }
                        supertonicStatus.text = "SuperTonic 3: ${models.detail}$engineLine"
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
