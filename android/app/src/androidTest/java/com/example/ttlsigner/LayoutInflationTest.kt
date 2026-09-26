package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import androidx.appcompat.view.ContextThemeWrapper
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.example.ttlsigner.ui.EmptyStateView
import com.example.ttlsigner.ui.SectionHeaderView
import com.example.ttlsigner.ui.SettingRowView
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
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
                R.id.pauseButton,
                R.id.clearButton,
                R.id.searchButton,
                R.id.filterButton,
                R.id.displayButton,
                R.id.countsText,
                R.id.awardText,
                R.id.searchBar,
                R.id.searchInput,
                R.id.closeSearchButton,
                R.id.filterPanel,
                R.id.filterBox,
                R.id.selectAllButton,
                R.id.clearFilterButton,
                R.id.eventsCard,
                R.id.eventsEmpty,
                R.id.eventList,
            ),
        )
    }

    @Test
    fun eventsToolbarButtonsAreIconButtons() {
        assertViewType(
            R.layout.fragment_events,
            R.id.pauseButton,
            com.google.android.material.button.MaterialButton::class.java,
        )
        assertViewType(
            R.layout.fragment_events,
            R.id.clearButton,
            com.google.android.material.button.MaterialButton::class.java,
        )
        assertViewType(
            R.layout.fragment_events,
            R.id.searchButton,
            com.google.android.material.button.MaterialButton::class.java,
        )
        assertViewType(
            R.layout.fragment_events,
            R.id.filterButton,
            com.google.android.material.button.MaterialButton::class.java,
        )
        assertViewType(
            R.layout.fragment_events,
            R.id.displayButton,
            com.google.android.material.button.MaterialButton::class.java,
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
                R.id.usernameInput,
                R.id.connectButton,
                R.id.disconnectButton,
                R.id.setupProgress,
                R.id.feedCard,
                R.id.feedButton,
                R.id.randomButton,
                R.id.feedError,
                R.id.feedList,
                R.id.ttsEngineGroup,
                R.id.ttsEngineOff,
                R.id.ttsEngineDevice,
                R.id.ttsEngineSuper,
                R.id.ttsJoinsSwitch,
                R.id.supertonicStatus,
                R.id.supertonicProgress,
                R.id.downloadSupertonicButton,
                R.id.testSpeechButton,
                R.id.ttsControlsHint,
                R.id.ttsControlsRow,
                R.id.repeatSpeechButton,
                R.id.skipSpeechButton,
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

    @Test
    fun emptyStatesAreSharedMinimalistComponent() {
        assertViewType(R.layout.fragment_events, R.id.eventsEmpty, EmptyStateView::class.java)
        assertViewType(R.layout.fragment_points, R.id.boardEmpty, EmptyStateView::class.java)
        assertViewType(R.layout.fragment_actions, R.id.actionsEmpty, EmptyStateView::class.java)
    }

    @Test
    fun sectionHeadersAreSharedMinimalistComponent() {
        assertViewType(R.layout.fragment_points, R.id.ratesHeader, SectionHeaderView::class.java)
        assertViewType(R.layout.fragment_points, R.id.adjustHeader, SectionHeaderView::class.java)
        assertViewType(R.layout.fragment_setup, R.id.ttsHeader, SectionHeaderView::class.java)
        assertViewType(R.layout.fragment_setup, R.id.signerHeader, SectionHeaderView::class.java)
    }

    @Test
    fun setupValuesSitInsideSettingRows() {
        // SettingRowView redirects its XML children into a value slot; the
        // wired IDs must still resolve somewhere beneath it.
        for (id in listOf(R.id.nativeVersionValue, R.id.bundleCacheValue, R.id.guestValue)) {
            val root = inflate(R.layout.fragment_setup)
            var parent = root.findViewById<View>(id).parent
            var insideRow = false
            while (parent != null && parent !== root) {
                if (parent is SettingRowView) {
                    insideRow = true
                    break
                }
                parent = (parent as? View)?.parent
            }
            assertTrue(
                "missing SettingRowView above ${root.resources.getResourceEntryName(id)}",
                insideRow,
            )
        }
    }

    private fun assertViewType(layout: Int, id: Int, type: Class<out View>) {
        val root = inflate(layout)
        val found = root.findViewById<View>(id)
        assertNotNull(
            "missing view: ${root.resources.getResourceEntryName(id)}",
            found,
        )
        assertTrue(
            "${root.resources.getResourceEntryName(id)} is ${found.javaClass.simpleName}, " +
                "want ${type.simpleName}",
            type.isInstance(found),
        )
    }

    private fun assertIds(layout: Int, ids: List<Int>) {
        val view = inflate(layout)
        for (id in ids) {
            assertNotNull(
                "missing view: ${view.resources.getResourceEntryName(id)}",
                view.findViewById<View>(id),
            )
        }
    }

    private fun inflate(layout: Int): View {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = themedContext()
        // Inflate on main: animated widgets (ripples, button states) start
        // animators during inflation, which needs a Looper thread.
        var root: View? = null
        instrumentation.runOnMainSync {
            root = LayoutInflater.from(context).inflate(layout, null)
        }
        return root!!
    }

    private fun themedContext(): ContextThemeWrapper {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // The raw test context carries the system theme; wrap it in the app theme
        // so Material attributes resolve exactly as in the activity.
        return ContextThemeWrapper(instrumentation.targetContext, R.style.Theme_TtlSigner)
    }
}
