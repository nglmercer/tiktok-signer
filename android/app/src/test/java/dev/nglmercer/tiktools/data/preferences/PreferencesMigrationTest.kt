package dev.nglmercer.tiktools.data.preferences

import android.app.Application
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import dev.nglmercer.tiktools.tts.TtsEngine
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class PreferencesMigrationTest {
    @Test
    fun legacySpeechRatesAndRowPreferencesMigrateOnce() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        context
            .getSharedPreferences("tiktools_tts", 0)
            .edit()
            .putBoolean("enabled", true)
            .putBoolean("joins", true)
            .commit()
        context
            .getSharedPreferences("tiktools_points", 0)
            .edit()
            .putString("config", "currency=XP;chat=7.0,true;level=50.0")
            .commit()
        context
            .getSharedPreferences("tiktools_events_display", 0)
            .edit()
            .putBoolean("compact", false)
            .putBoolean("time", false)
            .commit()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            val repo = PreferencesRepository(context, scope)
            val first = repo.values.first()
            assertEquals(TtsEngine.DEVICE, first.engine)
            assertTrue(first.speech.joins)
            assertEquals("XP", first.rewards.currencyName)
            assertEquals(7.0, first.rewards.pointsPerChat, 0.0)
            assertFalse(first.display.compact)
            assertFalse(first.display.showTime)
            repo.setEngine(TtsEngine.SUPERTONIC)
            repo.setSpeech(first.speech.copy(chat = false))
            repo.rememberCreator("creator")
            repo.rememberCreator("another")
            repo.rememberCreator("creator")
            val saved = repo.values.first()
            assertEquals(TtsEngine.SUPERTONIC, saved.engine)
            assertFalse(saved.speech.chat)
            assertTrue(saved.speech.joins)
            assertEquals(listOf("creator", "another"), saved.recentCreators)
        } finally {
            scope.cancel()
        }
    }
}
