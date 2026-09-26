package com.example.ttlsigner

import androidx.test.core.app.ActivityScenario
import androidx.test.espresso.Espresso.onView
import androidx.test.espresso.action.ViewActions.click
import androidx.test.espresso.action.ViewActions.scrollTo
import androidx.test.espresso.matcher.ViewMatchers.withId
import androidx.test.espresso.matcher.ViewMatchers.withText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Full UI drive: Setup feed → tap a room → connect → Events render →
 * disconnect. Needs network and a visible screen; the feed must be non-empty.
 */
@RunWith(AndroidJUnit4::class)
class UiFlowTest {

    @Test
    fun connectStreamsEventsIntoTheUiAndDisconnectStopsThem() {
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { activity = it }
        try {
            onView(withText(R.string.tab_setup)).perform(click())
            onView(withId(R.id.feedButton)).perform(scrollTo(), click())
            assertTrue("feed never rendered", waitFor(30_000) { activityFeedRows() > 0 })

            tapFirstFeedRow()
            // Tap selects, resolves, and signs (~10 s with the one-time bundle parse).
            assertTrue("room never resolved", waitFor(45_000) { outputText().contains("room=") })

            onView(withId(R.id.connectButton)).perform(scrollTo(), click())
            assertTrue("stream never opened", waitFor(30_000) { outputText().contains("LIVE open:") })

            onView(withText(R.string.tab_events)).perform(click())
            assertTrue("no events rendered", waitFor(45_000) { activityEventRows() > 0 })

            onView(withText(R.string.tab_setup)).perform(click())
            onView(withId(R.id.disconnectButton)).perform(scrollTo(), click())
            assertTrue("disconnect never landed", waitFor(15_000) {
                outputText().contains("disconnected in")
            })
        } finally {
            scenario.close()
        }
    }

    /** Poll [probe] on the UI thread until true or [timeoutMs] elapses. */
    private fun waitFor(timeoutMs: Long, probe: () -> Boolean): Boolean {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val end = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < end) {
            var result = false
            instrumentation.runOnMainSync { result = runCatching { probe() }.getOrDefault(false) }
            if (result) return true
            Thread.sleep(500)
        }
        var result = false
        instrumentation.runOnMainSync { result = runCatching { probe() }.getOrDefault(false) }
        return result
    }

    private var activity: MainActivity? = null

    private fun currentActivity(): MainActivity {
        if (activity == null) {
            // Re-fetch: set by the scenario below on first use.
            throw IllegalStateException("no activity")
        }
        return activity!!
    }

    private fun activityFeedRows(): Int =
        currentActivity().findViewById<androidx.recyclerview.widget.RecyclerView>(R.id.feedList)
            .adapter?.itemCount ?: 0

    private fun activityEventRows(): Int =
        currentActivity().findViewById<androidx.recyclerview.widget.RecyclerView>(R.id.eventList)
            .adapter?.itemCount ?: 0

    private fun outputText(): String =
        currentActivity().findViewById<android.widget.TextView>(R.id.output).text.toString()

    private fun tapFirstFeedRow() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.runOnMainSync {
            val list = currentActivity()
                .findViewById<androidx.recyclerview.widget.RecyclerView>(R.id.feedList)
            val holder = list.findViewHolderForAdapterPosition(0)
                ?: throw IllegalStateException("no feed row to tap")
            holder.itemView.performClick()
        }
    }
}
