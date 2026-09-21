package com.example.ttlsigner

import android.view.LayoutInflater
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
    fun mainLayoutInflatesWithEveryWiredId() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // The raw test context carries the system theme; wrap it in the app theme
        // so AppCompat attributes resolve exactly as in the activity.
        val context = ContextThemeWrapper(
            instrumentation.targetContext, R.style.Theme_TtlSigner)
        // Inflate on main: animated widgets (ripples, button states) start
        // animators during inflation, which needs a Looper thread.
        var root: android.view.View? = null
        instrumentation.runOnMainSync {
            root = LayoutInflater.from(context).inflate(R.layout.activity_main, null)
        }
        val view = root!!
        val ids = listOf(
            R.id.handleInput,
            R.id.resolveButton,
            R.id.signButton,
            R.id.feedButton,
            R.id.randomButton,
            R.id.connectButton,
            R.id.disconnectButton,
            R.id.progress,
            R.id.feedHeader,
            R.id.feedTitle,
            R.id.feedChevron,
            R.id.feedList,
            R.id.eventsHeader,
            R.id.eventsTitle,
            R.id.eventsChevron,
            R.id.eventList,
            R.id.logHeader,
            R.id.logChevron,
            R.id.copyButton,
            R.id.clearButton,
            R.id.outputScroll,
            R.id.output,
        )
        for (id in ids) {
            assertNotNull(
                "missing view: ${context.resources.getResourceEntryName(id)}",
                view.findViewById<android.view.View>(id),
            )
        }
    }

    @Test
    fun rowLayoutsInflate() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val inflater = LayoutInflater.from(
            ContextThemeWrapper(instrumentation.targetContext, R.style.Theme_TtlSigner))
        var feed: android.view.View? = null
        var eventRow: android.view.View? = null
        instrumentation.runOnMainSync {
            feed = inflater.inflate(R.layout.item_feed, null)
            eventRow = inflater.inflate(R.layout.item_event, null)
        }
        assertNotNull(feed!!.findViewById<android.view.View>(R.id.roomLine))
        assertNotNull(feed!!.findViewById<android.view.View>(R.id.titleLine))
        // The event row IS its TextView.
        assertNotNull(eventRow)
    }
}
