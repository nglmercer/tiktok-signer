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
    fun eventsTabInflatesWithEveryWiredId() {
        assertIds(
            R.layout.fragment_events,
            listOf(
                R.id.statusPill,
                R.id.countsText,
                R.id.awardText,
                R.id.filterChips,
                R.id.searchInput,
                R.id.pauseButton,
                R.id.clearButton,
                R.id.eventsCard,
                R.id.eventsEmpty,
                R.id.eventList,
            ),
        )
    }

    @Test
    fun pointsTabInflatesWithEveryWiredId() {
        assertIds(
            R.layout.fragment_points,
            listOf(
                R.id.boardCard,
                R.id.boardEmpty,
                R.id.boardList,
                R.id.ratesBox,
                R.id.currencyInput,
                R.id.levelInput,
                R.id.saveRatesButton,
                R.id.adjustUser,
                R.id.adjustAmount,
                R.id.adjustButton,
                R.id.resetButton,
            ),
        )
    }

    @Test
    fun actionsTabInflatesWithEveryWiredId() {
        assertIds(
            R.layout.fragment_actions,
            listOf(
                R.id.actionsCard,
                R.id.actionsEmpty,
                R.id.actionsList,
                R.id.addActionButton,
                R.id.runsCard,
                R.id.runsText,
            ),
        )
    }

    @Test
    fun setupTabInflatesWithEveryWiredId() {
        assertIds(
            R.layout.fragment_setup,
            listOf(
                R.id.statusPill,
                R.id.roomInput,
                R.id.connectButton,
                R.id.disconnectButton,
                R.id.setupProgress,
                R.id.feedCard,
                R.id.feedButton,
                R.id.randomButton,
                R.id.feedError,
                R.id.feedList,
                R.id.signCard,
                R.id.handleInput,
                R.id.resolveButton,
                R.id.signButton,
                R.id.resultPlaceholder,
                R.id.resultRoom,
                R.id.resultUser,
                R.id.resultSummary,
                R.id.resultLatency,
                R.id.resultError,
                R.id.ttsSwitch,
                R.id.ttsJoinsSwitch,
                R.id.nativeVersionValue,
                R.id.bundleCacheValue,
                R.id.redownloadButton,
                R.id.clearCacheButton,
                R.id.guestValue,
                R.id.resetSessionButton,
                R.id.logConsole,
                // The console's internals: UiFlowTest reads the log through R.id.output.
                R.id.copyButton,
                R.id.clearButton,
                R.id.outputScroll,
                R.id.output,
                R.id.aboutValue,
            ),
        )
    }

    @Test
    fun actionEditorInflatesWithEveryWiredId() {
        assertIds(
            R.layout.dialog_action_edit,
            listOf(
                R.id.actionNameInput,
                R.id.triggerChips,
                R.id.methodSpinner,
                R.id.urlInput,
                R.id.bodyInput,
                R.id.cooldownInput,
            ),
        )
    }

    @Test
    fun rowLayoutsInflate() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val inflater = LayoutInflater.from(themedContext())
        var feed: View? = null
        var eventRow: View? = null
        var pointsRow: View? = null
        var actionRow: View? = null
        var rateRow: View? = null
        instrumentation.runOnMainSync {
            feed = inflater.inflate(R.layout.item_feed, null)
            eventRow = inflater.inflate(R.layout.item_event, null)
            pointsRow = inflater.inflate(R.layout.item_points, null)
            actionRow = inflater.inflate(R.layout.item_action, null)
            rateRow = inflater.inflate(R.layout.item_rate, null)
        }
        assertNotNull(feed!!.findViewById<View>(R.id.roomLine))
        assertNotNull(feed!!.findViewById<View>(R.id.titleLine))
        assertNotNull(eventRow!!.findViewById<View>(R.id.eventBadge))
        assertNotNull(eventRow!!.findViewById<View>(R.id.eventBody))
        assertNotNull(eventRow!!.findViewById<View>(R.id.eventTime))
        assertNotNull(pointsRow!!.findViewById<View>(R.id.rankText))
        assertNotNull(pointsRow!!.findViewById<View>(R.id.userText))
        assertNotNull(pointsRow!!.findViewById<View>(R.id.balanceText))
        assertNotNull(pointsRow!!.findViewById<View>(R.id.levelText))
        assertNotNull(actionRow!!.findViewById<View>(R.id.actionName))
        assertNotNull(actionRow!!.findViewById<View>(R.id.actionDetail))
        assertNotNull(actionRow!!.findViewById<View>(R.id.actionToggle))
        assertNotNull(actionRow!!.findViewById<View>(R.id.actionEdit))
        assertNotNull(actionRow!!.findViewById<View>(R.id.actionTest))
        assertNotNull(actionRow!!.findViewById<View>(R.id.actionDelete))
        assertNotNull(rateRow!!.findViewById<View>(R.id.rateLabel))
        assertNotNull(rateRow!!.findViewById<View>(R.id.rateInput))
        assertNotNull(rateRow!!.findViewById<View>(R.id.rateEnabled))
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
