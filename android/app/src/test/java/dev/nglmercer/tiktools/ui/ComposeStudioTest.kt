package dev.nglmercer.tiktools.ui

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import dev.nglmercer.tiktools.app.StudioNavigation
import dev.nglmercer.tiktools.core.model.*
import dev.nglmercer.tiktools.feature.events.*
import dev.nglmercer.tiktools.feature.home.*
import dev.nglmercer.tiktools.live.LiveSessionState
import dev.nglmercer.tiktools.ui.theme.TikToolsTheme
import java.io.File
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class, qualifiers = "w393dp-h852dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ComposeStudioTest {
    @get:Rule val compose = createAndroidComposeRule<androidx.activity.ComponentActivity>()

    @Test
    fun homeConnectHasAccessibleTouchTargetAndOfflineSnapshot() {
        var requested: HomeAction? = null
        compose.setContent { TikToolsTheme { HomeScreen(HomeUiState(), { requested = it }) } }
        compose.onNodeWithTag("connect-button").assertIsNotEnabled().assertHeightIsAtLeast(48.dp)
        compose.onNodeWithText("TikTok username").performTextInput("@creator")
        compose.onNodeWithTag("connect-button").assertIsEnabled().performClick()
        assertEquals(HomeAction.Connect("@creator"), requested)
        screenshot("home-offline")
    }

    @Test
    fun liveHomeSnapshotAndDisconnect() {
        var disconnected = false
        compose.setContent {
            TikToolsTheme {
                HomeScreen(
                    HomeUiState(
                        connection =
                            LiveSessionState.Connected(
                                Creator(
                                    "creator",
                                    "123",
                                    "Creator",
                                    "Minecraft LIVE",
                                    viewers = 1284,
                                )
                            ),
                        stats =
                            dev.nglmercer.tiktools.data.events.LiveStats(
                                events = 342,
                                gifts = 18,
                                likes = 2400,
                            ),
                    ),
                    { if (it == HomeAction.Disconnect) disconnected = true },
                )
            }
        }
        compose.onNodeWithText("LIVE").assertIsDisplayed()
        compose.onNodeWithText("Disconnect").performClick()
        assertTrue(disconnected)
        screenshot("home-live")
    }

    @Test
    fun navigationMakesSettingsSecondaryAndReturnsToHome() {
        compose.setContent {
            TikToolsTheme {
                StudioNavigation(
                    home = { Text("Home content") },
                    events = { Text("Events content") },
                    actions = { Text("Actions content") },
                    rewards = { Text("Rewards content") },
                    settings = { Text("Settings content") },
                    speech = { Text("Speech content") },
                    diagnostics = { Text("Diagnostics content") },
                )
            }
        }
        compose.onNodeWithText("Home content").assertIsDisplayed()
        compose.onNodeWithText("Events").performClick()
        compose.onNodeWithText("Events content").assertIsDisplayed()
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Settings content").assertIsDisplayed()
        compose.onNodeWithContentDescription("Back").performClick()
        compose.onNodeWithText("Events content").assertIsDisplayed()
        compose.onNodeWithText("Home").performClick()
        compose.onNodeWithText("Home content").assertIsDisplayed()
    }

    @Test
    fun eventDetailsCopyJsonAndPauseRemainAvailable() {
        var paused = false
        val raw =
            "{\"type\":\"gift\",\"gift_name\":\"Rose\",\"repeat_count\":5,\"diamond_count\":1,\"user\":{\"unique_id\":\"creator\",\"nickname\":\"Creator\"}}"
        compose.setContent {
            TikToolsTheme {
                EventsScreen(
                    EventsUiState(events = listOf(EventParser.parse(raw))),
                    {},
                    {},
                    { paused = it },
                    {},
                )
            }
        }
        compose.onNodeWithContentDescription("More event options").performClick()
        compose.onNodeWithText("Pause events").performClick()
        assertTrue(paused)
        compose.onNodeWithText("Creator").performClick()
        compose.onNodeWithText("Copy JSON").assertIsDisplayed()
        screenshot("event-detail")
    }

    @Test
    @Config(qualifiers = "w840dp-h900dp")
    fun tabletUsesRailAndPreservesNavigation() {
        compose.setContent {
            TikToolsTheme {
                StudioNavigation(
                    home = { HomeScreen(HomeUiState(), {}) },
                    events = { Text("Tablet events") },
                    actions = { Text("Tablet actions") },
                    rewards = { Text("Tablet rewards") },
                    settings = { Text("Tablet settings") },
                    speech = { Text("Tablet speech") },
                    diagnostics = { Text("Tablet diagnostics") },
                )
            }
        }
        val home = compose.onNodeWithText("Home").fetchSemanticsNode().boundsInRoot
        val rewards = compose.onNodeWithText("Rewards").fetchSemanticsNode().boundsInRoot
        assertEquals(home.center.x, rewards.center.x, 1f)
        assertTrue(rewards.top > home.bottom)
        screenshot("tablet-home")
        compose.onNodeWithText("Events").performClick()
        compose.onNodeWithText("Tablet events").assertIsDisplayed()
    }

    private fun screenshot(name: String) {
        compose.waitForIdle()
        val output = File(System.getProperty("screenshotDir") ?: "build/screenshots")
        output.mkdirs()
        compose.runOnIdle {
            val dialog =
                org.robolectric.shadows.ShadowDialog.getLatestDialog()?.takeIf { it.isShowing }
            val view = dialog?.window?.decorView ?: compose.activity.window.decorView
            val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
            view.draw(android.graphics.Canvas(bitmap))
            File(output, "$name.png").outputStream().use {
                bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
            }
            bitmap.recycle()
        }
    }
}
