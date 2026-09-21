package com.example.ttlsigner

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.fragment.app.Fragment
import androidx.fragment.app.activityViewModels
import androidx.lifecycle.lifecycleScope
import com.google.android.material.progressindicator.LinearProgressIndicator
import kotlinx.coroutines.launch

/** Settings tab: signer, bundle cache, guest session, log, about. */
class SettingsFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_settings, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val nativeVersionValue: TextView = view.findViewById(R.id.nativeVersionValue)
        val userAgentValue: TextView = view.findViewById(R.id.userAgentValue)
        val bundleVersionValue: TextView = view.findViewById(R.id.bundleVersionValue)
        val bundleCacheValue: TextView = view.findViewById(R.id.bundleCacheValue)
        val redownloadButton: Button = view.findViewById(R.id.redownloadButton)
        val clearCacheButton: Button = view.findViewById(R.id.clearCacheButton)
        val settingsProgress: LinearProgressIndicator = view.findViewById(R.id.settingsProgress)
        val guestValue: TextView = view.findViewById(R.id.guestValue)
        val resetSessionButton: Button = view.findViewById(R.id.resetSessionButton)
        val aboutValue: TextView = view.findViewById(R.id.aboutValue)

        fun refreshGuest() {
            val names = vm.guestCookieNames()
            guestValue.text = names.ifEmpty {
                listOf(getString(R.string.settings_no_cookies))
            }.joinToString(", ")
        }

        fun refreshBundle(state: SessionViewModel.BundleUiState) {
            bundleVersionValue.text = "${state.version} · sha ${state.shaShort}…"
            val cache = state.cache
            bundleCacheValue.text = cache?.let {
                "${it.sizeBytes / 1024} KB · ${age(it.ageMs)} · " +
                    if (it.shaOk) "digest OK" else "DIGEST MISMATCH"
            } ?: getString(R.string.settings_bundle_none)
        }

        viewLifecycleOwner.lifecycleScope.launch {
            nativeVersionValue.text = vm.nativeVersion()
            userAgentValue.text = vm.userAgent()
            refreshBundle(vm.bundleState())
            refreshGuest()
            aboutValue.text =
                "${getString(R.string.settings_about_text)}\n\nApp ${appVersion()}"
        }

        fun setBundleBusy(busy: Boolean) {
            settingsProgress.visibility = if (busy) View.VISIBLE else View.GONE
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
        view.findViewById<Button>(R.id.copyLogButton).setOnClickListener {
            val text = Logger.snapshot()
            val clip = ClipData.newPlainText("ttl-signer log", text)
            (requireContext().getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager)
                .setPrimaryClip(clip)
            toast("log copied (${text.length} chars)")
        }
        view.findViewById<Button>(R.id.clearLogButton).setOnClickListener {
            vm.clearLog()
            toast("log cleared")
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
