package com.example.ttlsigner

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.fragment.app.Fragment
import androidx.fragment.app.FragmentActivity
import androidx.viewpager2.adapter.FragmentStateAdapter
import androidx.viewpager2.widget.ViewPager2
import com.google.android.material.color.DynamicColors
import com.google.android.material.tabs.TabLayout
import com.google.android.material.tabs.TabLayoutMediator

/**
 * TikTools Studio: a small Android studio for TikTok LIVE. Four tabs share one
 * [SessionViewModel], which owns the connection and fans every live event out
 * to the reader, the points bank, the fetch-only actions, and the speaker:
 *
 * - Events: the reader — filter chips, search, pause, per-type counts.
 * - Points: the SQLite leaderboard, rates, manual adjust, reset.
 * - Actions: fetch-only automations per event kind, plus the run log.
 * - Setup: connect/disconnect, live feed, resolve + sign, TTS toggles,
 *   signer/session maintenance, debug console.
 *
 * Signing and the event stream run on-device through the reused Rust core
 * (`libttl_sign_mobile.so` over JNI). The screen may show a full signed URL;
 * logcat only ever gets its summary — see [Logger.summarizeSignedUrl].
 */
class MainActivity : AppCompatActivity() {

    // Android 13+ gates the keep-alive notification behind a runtime grant; the
    // stream still connects without it, but the background hold needs the tap.
    private val notificationGrant =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Material You recolor on Android 12+; a no-op below.
        DynamicColors.applyToActivityIfAvailable(this)
        setContentView(R.layout.activity_main)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            notificationGrant.launch(Manifest.permission.POST_NOTIFICATIONS)
        }

        val pager: ViewPager2 = findViewById(R.id.viewPager)
        pager.adapter = TabsAdapter(this)
        // Keep one neighbor tab's views attached so flows keep collecting.
        pager.offscreenPageLimit = 1
        val tabs: TabLayout = findViewById(R.id.tabLayout)
        val titles = listOf(
            getString(R.string.tab_events),
            getString(R.string.tab_points),
            getString(R.string.tab_actions),
            getString(R.string.tab_setup),
        )
        // Icons for every tab; text stays for accessibility and tests.
        val icons = listOf(
            R.drawable.ic_ev_chat,
            R.drawable.ic_ev_gift,
            R.drawable.ic_ev_share,
            R.drawable.ic_tune,
        )
        TabLayoutMediator(tabs, pager) { tab, position ->
            tab.text = titles[position]
            tab.setIcon(icons[position])
        }.attach()
    }

    private class TabsAdapter(activity: FragmentActivity) : FragmentStateAdapter(activity) {
        override fun getItemCount(): Int = 4

        override fun createFragment(position: Int): Fragment = when (position) {
            0 -> EventsFragment()
            1 -> PointsFragment()
            2 -> ActionsFragment()
            else -> SetupFragment()
        }
    }
}
