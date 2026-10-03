package dev.nglmercer.tiktools.app

import android.content.Context
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.*
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import dev.nglmercer.tiktools.feature.actions.*
import dev.nglmercer.tiktools.feature.events.*
import dev.nglmercer.tiktools.feature.home.*
import dev.nglmercer.tiktools.feature.rewards.*
import dev.nglmercer.tiktools.feature.settings.*

class StudioViewModelFactory(private val container: AppContainer, private val context: Context) :
    ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T =
        when (modelClass) {
            HomeViewModel::class.java -> HomeViewModel(container)
            EventsViewModel::class.java -> EventsViewModel(container)
            ActionsViewModel::class.java -> ActionsViewModel(container)
            RewardsViewModel::class.java -> RewardsViewModel(container)
            SettingsViewModel::class.java ->
                SettingsViewModel(container, context.applicationContext)
            else -> error("Unknown screen ViewModel: $modelClass")
        }
            as T
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TikToolsApp(container: AppContainer) {
    val context = LocalContext.current
    val factory = remember(container) { StudioViewModelFactory(container, context) }
    var speechSheet by remember { mutableStateOf(false) }
    StudioNavigation(
        home = {
            val vm: HomeViewModel = viewModel(factory = factory)
            val state = vm.state.collectAsStateWithLifecycle().value
            val error = vm.error.collectAsStateWithLifecycle().value
            HomeScreen(
                state,
                { action ->
                    when (action) {
                        is HomeAction.Connect -> vm.connect(action.username)
                        is HomeAction.SelectCreator ->
                            vm.connect(action.creator.uniqueId, action.creator)
                        HomeAction.Disconnect -> vm.disconnect()
                        HomeAction.Refresh -> vm.refreshFeed()
                        HomeAction.Random -> vm.random()
                        HomeAction.ToggleTts -> vm.toggleSpeech()
                        HomeAction.Speech -> speechSheet = true
                    }
                },
                error,
                vm::dismissError,
            )
        },
        events = {
            val vm: EventsViewModel = viewModel(factory = factory)
            EventsScreen(
                vm.state.collectAsStateWithLifecycle().value,
                vm::filter,
                vm::display,
                vm::pause,
                vm::clear,
            )
        },
        actions = {
            val vm: ActionsViewModel = viewModel(factory = factory)
            ActionsScreen(vm)
        },
        rewards = {
            val vm: RewardsViewModel = viewModel(factory = factory)
            RewardsScreen(vm)
        },
        settings = { navigate ->
            val vm: SettingsViewModel = viewModel(factory = factory)
            SettingsScreen(vm, navigate)
        },
        speech = {
            val vm: SettingsViewModel = viewModel(factory = factory)
            SpeechScreen(vm)
        },
        diagnostics = {
            val vm: SettingsViewModel = viewModel(factory = factory)
            DiagnosticsScreen(vm)
        },
    )
    if (speechSheet) {
        val vm: SettingsViewModel = viewModel(key = "quick-speech", factory = factory)
        val prefs = vm.preferences.collectAsStateWithLifecycle().value
        val last = vm.lastSpoken.collectAsStateWithLifecycle().value
        ModalBottomSheet(onDismissRequest = { speechSheet = false }) {
            Column(
                Modifier.verticalScroll(rememberScrollState())
                    .padding(16.dp)
                    .navigationBarsPadding()
            ) {
                Text("Speech", style = MaterialTheme.typography.headlineMedium)
                SpeechControls(prefs, vm::engine, vm::speech, vm::repeat, vm::skip, last != null)
                Text(
                    "Offline model download and engine details are in Settings → Speech",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}
