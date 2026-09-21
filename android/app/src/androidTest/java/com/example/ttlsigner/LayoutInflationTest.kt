package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import androidx.appcompat.view.ContextThemeWrapper
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Layouts inflate and every ID the code touches resolves. Headless by design:
 * no activity, no screen — this catches broken XML even when the device
 * is locked. Visible behavior lives in [UiFlowTest].
 */
@RunWith(AndroidJUnit4::class)
class LayoutInflationTest {

    @Test
    fun shellInflatesWithTabsAndPager() {
        assertIds(
            R.layout.activity_main,
            listOf(R.id.toolbar, R.id.tabLayout, R.id.viewPager),
        )
    }

    @Test
    fun liveTabInflatesWithEveryWiredId() {
        assertIds(
            R.layout.fragment_live,
            listOf(
                R.id.statusPill,
                R.id.roomInput,
                R.id.connectButton,
                R.id.disconnectButton,
                R.id.liveProgress,
                R.id.feedCard,
                R.id.feedButton,
                R.id.randomButton,
                R.id.feedError,
                R.id.feedList,
                R.id.eventsCard,
                R.id.eventsEmpty,
                R.id.eventsClearButton,
                R.id.eventList,
            ),
        )
    }

    @Test
    fun signTabInflatesWithEveryWiredId() {
        assertIds(
            R.layout.fragment_sign,
            listOf(
                R.id.handleInput,
                R.id.resolveButton,
                R.id.signButton,
                R.id.signProgress,
                R.id.resultCard,
                R.id.resultPlaceholder,
                R.id.resultRoom,
                R.id.resultUser,
                R.id.resultSummary,
                R.id.resultLatency,
                R.id.resultError,
                R.id.logConsole,
                // The console's internals: UiFlowTest reads the log through R.id.output.
                R.id.copyButton,
                R.id.clearButton,
                R.id.outputScroll,
                R.id.output,
            ),
        )
    }

    @Test
    fun settingsTabInflatesWithEveryWiredId() {
        assertIds(
            R.layout.fragment_settings,
            listOf(
                R.id.nativeVersionValue,
                R.id.userAgentValue,
                R.id.bundleVersionValue,
                R.id.bundleCacheValue,
                R.id.redownloadButton,
                R.id.clearCacheButton,
                R.id.settingsProgress,
                R.id.guestValue,
                R.id.resetSessionButton,
                R.id.copyLogButton,
                R.id.clearLogButton,
                R.id.aboutValue,
            ),
        )
    }

    @Test
    fun rowLayoutsInflate() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val inflater = LayoutInflater.from(themedContext())
        var feed: View? = null
        var eventRow: View? = null
        instrumentation.runOnMainSync {
            feed = inflater.inflate(R.layout.item_feed, null)
            eventRow = inflater.inflate(R.layout.item_event, null)
        }
        assertNotNull(feed!!.findViewById<View>(R.id.roomLine))
        assertNotNull(feed!!.findViewById<View>(R.id.titleLine))
        // The event row IS its TextView.
        assertNotNull(eventRow)
    }

    private fun assertIds(layout: Int, ids: List<Int>) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = themedContext()
        // Inflate on main: animated widgets (ripples, button states) start
        // animators during inflation, which needs a Looper thread.
        var root: View? = null
        instrumentation.runOnMainSync {
            root = LayoutInflater.from(context).inflate(layout, null)
        }
        val view = root!!
        for (id in ids) {
            assertNotNull(
                "missing view: ${context.resources.getResourceEntryName(id)}",
                view.findViewById<View>(id),
            )
        }
    }

    private fun themedContext(): ContextThemeWrapper {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // The raw test context carries the system theme; wrap it in the app theme
        // so Material attributes resolve exactly as in the activity.
        return ContextThemeWrapper(instrumentation.targetContext, R.style.Theme_TtlSigner)
    }
}
