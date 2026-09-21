package com.example.ttlsigner

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.fragment.app.Fragment
import androidx.fragment.app.FragmentActivity
import androidx.viewpager2.adapter.FragmentStateAdapter
import androidx.viewpager2.widget.ViewPager2
import com.google.android.material.color.DynamicColors
import com.google.android.material.tabs.TabLayout
import com.google.android.material.tabs.TabLayoutMediator

/**
 * Example app for on-device signing through the reused Rust core:
 * `libttl_sign_mobile.so` over JNI. Three tabs share one [SessionViewModel]:
 *
 * - Live: connect the event stream, browse the live feed, watch events land.
 * - Sign: resolve a handle (or type a numeric room id to skip the network),
 *   sign the socket URL, read the debug console.
 * - Settings: native core, bundle cache, guest session, log, about.
 *
 * The screen may show a full signed URL; logcat only ever gets its summary —
 * see [Logger.summarizeSignedUrl].
 */
class MainActivity : AppCompatActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Material You recolor on Android 12+; a no-op below.
        DynamicColors.applyToActivityIfAvailable(this)
        setContentView(R.layout.activity_main)

        val pager: ViewPager2 = findViewById(R.id.viewPager)
        pager.adapter = TabsAdapter(this)
        // Keep the neighbor tab's views (e.g. the Sign console) attached so the
        // debug log renders even while another tab is in front.
        pager.offscreenPageLimit = 1
        val tabs: TabLayout = findViewById(R.id.tabLayout)
        val titles = listOf(
            getString(R.string.tab_live),
            getString(R.string.tab_sign),
            getString(R.string.tab_settings),
        )
        TabLayoutMediator(tabs, pager) { tab, position -> tab.text = titles[position] }.attach()
    }

    private class TabsAdapter(activity: FragmentActivity) : FragmentStateAdapter(activity) {
        override fun getItemCount(): Int = 3

        override fun createFragment(position: Int): Fragment = when (position) {
            0 -> LiveFragment()
            1 -> SignFragment()
            else -> SettingsFragment()
        }
    }
}
