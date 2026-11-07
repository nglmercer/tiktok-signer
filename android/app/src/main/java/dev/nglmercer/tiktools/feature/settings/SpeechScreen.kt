package dev.nglmercer.tiktools.feature.settings

import android.content.Intent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalLocale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nglmercer.tiktools.R
import dev.nglmercer.tiktools.data.preferences.*
import dev.nglmercer.tiktools.tts.TtsEngine
import dev.nglmercer.tiktools.tts.supertonic.LanguageMode
import dev.nglmercer.tiktools.tts.supertonic.QualityPreset
import dev.nglmercer.tiktools.tts.supertonic.SupertonicLanguages
import dev.nglmercer.tiktools.tts.supertonic.SupertonicModels
import dev.nglmercer.tiktools.tts.supertonic.SupertonicQuality
import dev.nglmercer.tiktools.tts.supertonic.SupertonicSpeaker
import dev.nglmercer.tiktools.ui.components.*
import java.util.Locale
import kotlinx.coroutines.delay

/** Engine choice, LIVE announce toggles and chat-name behaviour. Shared with the home quick sheet. */
@Composable
fun SpeechControls(
    prefs: UserPreferences,
    onEngine: (TtsEngine) -> Unit,
    onSpeech: (SpeechPreferences) -> Unit,
    onRepeat: () -> Unit,
    onSkip: () -> Unit,
    canRepeat: Boolean,
) {
    for (engine in TtsEngine.entries) Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable { onEngine(engine) },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(prefs.engine == engine, onClick = { onEngine(engine) })
        Text(
            when (engine) {
                TtsEngine.OFF -> tr(R.string.speech_off)
                TtsEngine.DEVICE -> tr(R.string.speech_device_voice)
                TtsEngine.SUPERTONIC -> tr(R.string.speech_engine_super)
            }
        )
    }
    if (prefs.engine != TtsEngine.OFF)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onRepeat, enabled = canRepeat) {
                Text(tr(R.string.repeat_speech))
            }
            OutlinedButton(onClick = onSkip) { Text(tr(R.string.skip_speech)) }
        }
    SectionHeader(tr(R.string.speech_announce))
    val p = prefs.speech
    var chatPrefix by rememberSaveable { mutableStateOf(p.chatPrefix) }
    val currentSpeech by rememberUpdatedState(p)
    LaunchedEffect(chatPrefix) {
        delay(300)
        if (chatPrefix != currentSpeech.chatPrefix)
            onSpeech(currentSpeech.copy(chatPrefix = chatPrefix))
    }
    SettingSwitch(tr(R.string.speech_chat), p.chat, { onSpeech(p.copy(chat = it)) })
    SettingSwitch(tr(R.string.speech_gifts), p.gifts, { onSpeech(p.copy(gifts = it)) })
    SettingSwitch(tr(R.string.speech_follows), p.follows, { onSpeech(p.copy(follows = it)) })
    SettingSwitch(tr(R.string.speech_shares), p.shares, { onSpeech(p.copy(shares = it)) })
    SettingSwitch(tr(R.string.speech_joins), p.joins, { onSpeech(p.copy(joins = it)) })
    SectionHeader(tr(R.string.speech_emote_cleanup))
    Text(
        tr(R.string.speech_emote_cleanup_hint),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    SettingSwitch(
        tr(R.string.speech_emote_remove_known),
        p.sanitizer.removeConfirmedEmotes,
        { onSpeech(p.copy(sanitizer = p.sanitizer.copy(removeConfirmedEmotes = it))) },
    )
    SettingSwitch(
        tr(R.string.speech_emote_skip_empty),
        p.sanitizer.skipEmptyMessages,
        { onSpeech(p.copy(sanitizer = p.sanitizer.copy(skipEmptyMessages = it))) },
        tr(R.string.speech_emote_skip_empty_hint),
    )
    SettingSwitch(
        tr(R.string.speech_emote_collapse),
        p.sanitizer.collapseWhitespace,
        { onSpeech(p.copy(sanitizer = p.sanitizer.copy(collapseWhitespace = it))) },
    )
    SettingSwitch(
        tr(R.string.speech_emote_remove_unicode),
        p.sanitizer.removeUnicodeEmoji,
        { onSpeech(p.copy(sanitizer = p.sanitizer.copy(removeUnicodeEmoji = it))) },
        tr(R.string.speech_emote_remove_unicode_hint),
    )
    SectionHeader(tr(R.string.speech_chat_name_mode))
    for (mode in ChatNameMode.entries) {
        val label =
            tr(
                when (mode) {
                    ChatNameMode.ALWAYS -> R.string.speech_chat_name_always
                    ChatNameMode.WHEN_CHANGED -> R.string.speech_chat_name_changed
                    ChatNameMode.NEVER -> R.string.speech_chat_name_never
                }
            )
        Row(
            Modifier.fillMaxWidth().heightIn(min = 52.dp).clickable {
                onSpeech(p.copy(chatNameMode = mode))
            },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(p.chatNameMode == mode, { onSpeech(p.copy(chatNameMode = mode)) })
            Text(label)
        }
    }
    if (p.chatNameMode == ChatNameMode.WHEN_CHANGED)
        Text(
            tr(R.string.speech_chat_name_changed_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    OutlinedTextField(
        value = chatPrefix,
        onValueChange = { chatPrefix = it.take(40) },
        modifier = Modifier.fillMaxWidth(),
        label = { Text(tr(R.string.speech_chat_prefix)) },
        placeholder = { Text(tr(R.string.speech_chat_prefix_placeholder)) },
        supportingText = { Text(tr(R.string.speech_chat_prefix_hint)) },
        singleLine = true,
    )
}

@Composable
fun SpeechScreen(vm: SettingsViewModel) {
    val prefs = vm.preferences.collectAsStateWithLifecycle().value
    val model = vm.models.collectAsStateWithLifecycle().value
    val voiceState = vm.voice.collectAsStateWithLifecycle().value
    val last = vm.lastSpoken.collectAsStateWithLifecycle().value
    val error = vm.error.collectAsStateWithLifecycle().value
    val context = LocalContext.current
    var deleting by remember { mutableStateOf(false) }
    var managingVoices by remember { mutableStateOf(false) }
    var advanced by remember { mutableStateOf(false) }
    var customText by rememberSaveable { mutableStateOf("") }
    val p = prefs.speech
    val testLine = tr(R.string.speech_test_line)

    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Text(tr(R.string.speech_title), style = MaterialTheme.typography.headlineMedium)
            ErrorBanner(error, vm::dismissError)
        }
        item { SpeechControls(prefs, vm::engine, vm::speech, vm::repeat, vm::skip, last != null) }
        item {
            HorizontalDivider()
            SectionHeader(tr(R.string.speech_engine_super))
            Text(tr(R.string.speech_offline_quality), style = MaterialTheme.typography.titleMedium)
            Text(
                tr(R.string.speech_model_info),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            StatusRow(model, voiceState)
            Text(model.detail)
            if (model.bytes > 0)
                Text(
                    tr(R.string.speech_model_bytes, localizedAmount(model.bytes / 1048576.0)),
                    style = MaterialTheme.typography.bodySmall,
                )
            ErrorBanner(model.error)
        }
        item {
            when {
                model.downloading || model.validating -> {
                    LinearProgressIndicator(
                        progress = { model.progress ?: 0f },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Text("${((model.progress ?: 0f) * 100).toInt()}%")
                    TextButton(onClick = vm::cancelDownload) {
                        Text(tr(R.string.speech_cancel_download))
                    }
                }
                model.ready ->
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(
                            onClick = vm::testVoice,
                            enabled = prefs.engine != TtsEngine.OFF,
                        ) {
                            Text(tr(R.string.speech_test_voice))
                        }
                        TextButton(onClick = vm::reloadModel) {
                            Text(tr(R.string.speech_reload_model))
                        }
                        TextButton(onClick = { deleting = true }) {
                            Text(tr(R.string.speech_delete_model))
                        }
                    }
                else -> Button(onClick = vm::download) { Text(tr(R.string.supertonic_download)) }
            }
        }
        if (model.ready) {
            item { HorizontalDivider() }
            item {
                VoiceSection(
                    selected = p.voice,
                    voices = model.voices,
                    downloaded = model.downloadedVoices,
                    enabled = prefs.engine != TtsEngine.OFF,
                    managing = managingVoices,
                    onToggleManage = { managingVoices = !managingVoices },
                    onSelect = vm::selectVoice,
                    onDownload = vm::downloadVoice,
                    onDelete = vm::deleteVoice,
                    onPreview = { name -> vm.previewVoice(testLine, name) },
                )
            }
            item { HorizontalDivider() }
            item { LanguageSection(p, vm.languages, vm::speech) }
            item { HorizontalDivider() }
            item { QualitySection(p, vm::speech) }
            item { SpeedSection(p, vm::speech) }
            item {
                HorizontalDivider()
                SectionHeader(tr(R.string.speech_preview))
                OutlinedTextField(
                    value = customText,
                    onValueChange = { customText = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text(tr(R.string.speech_custom_preview)) },
                    supportingText = { Text(tr(R.string.speech_custom_preview_hint)) },
                    minLines = 2,
                )
                Button(
                    onClick = { vm.previewVoice(customText.ifBlank { testLine }) },
                    enabled = prefs.engine != TtsEngine.OFF,
                ) {
                    Text(tr(R.string.speech_preview))
                }
            }
            item { HorizontalDivider() }
            item {
                AdvancedSection(
                    expanded = advanced,
                    onToggle = { advanced = !advanced },
                    prefs = p,
                    hasDiagnosticAudio = vm.diagnosticsDir().listFiles().orEmpty().any { it.extension == "wav" },
                    onUpdate = vm::updateSpeech,
                    onShare = {
                        vm.diagnosticShareIntent()
                            ?.let { intent ->
                                runCatching {
                                    context.startActivity(Intent.createChooser(intent, null))
                                }
                            }
                    },
                    onClearDiagnostics = vm::clearDiagnosticAudio,
                )
            }
        }
        item { Spacer(Modifier.height(24.dp)) }
    }

    if (deleting)
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text(tr(R.string.speech_delete_title)) },
            text = { Text(tr(R.string.speech_delete_message)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        vm.deleteModel()
                        deleting = false
                    }
                ) {
                    Text(tr(R.string.actions_delete))
                }
            },
            dismissButton = {
                TextButton(onClick = { deleting = false }) { Text(tr(R.string.cancel)) }
            },
        )
}

@Composable
private fun StatusRow(
    model: dev.nglmercer.tiktools.tts.TtsRepository.ModelState,
    state: SupertonicSpeaker.State,
) {
    val label =
        when {
            model.downloading -> tr(R.string.speech_status_downloading)
            model.validating -> tr(R.string.speech_status_validating)
            !model.ready -> tr(R.string.speech_status_not_installed)
            else ->
                when (state) {
                    SupertonicSpeaker.State.Idle -> tr(R.string.speech_status_idle)
                    SupertonicSpeaker.State.Loading -> tr(R.string.speech_status_loading)
                    SupertonicSpeaker.State.Ready -> tr(R.string.speech_status_ready)
                    SupertonicSpeaker.State.NotInstalled -> tr(R.string.speech_status_not_installed)
                    is SupertonicSpeaker.State.Generating -> tr(R.string.speech_status_generating)
                    is SupertonicSpeaker.State.Playing -> tr(R.string.speech_status_playing)
                    is SupertonicSpeaker.State.Error -> tr(R.string.speech_status_error)
                }
        }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            "${tr(R.string.speech_model_status)}: $label",
            style = MaterialTheme.typography.bodyMedium,
        )
        if (state is SupertonicSpeaker.State.Error)
            Text(
                state.detail,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
    }
}

@Composable
private fun VoiceSection(
    selected: String,
    voices: List<String>,
    downloaded: List<String>,
    enabled: Boolean,
    managing: Boolean,
    onToggleManage: () -> Unit,
    onSelect: (String) -> Unit,
    onDownload: (String) -> Unit,
    onDelete: (String) -> Unit,
    onPreview: (String) -> Unit,
) {
    SectionHeader(tr(R.string.speech_voice)) {
        TextButton(onClick = onToggleManage) {
            Text(tr(R.string.speech_manage_voices))
        }
    }
    for (name in voices) {
        val isDownloaded = name in downloaded
        Row(
            Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(enabled = enabled) {
                onSelect(name)
            },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected == name, onClick = { onSelect(name) })
            Column(Modifier.weight(1f)) {
                Text(name)
                Text(
                    tr(
                        if (isDownloaded) R.string.speech_voice_downloaded
                        else R.string.speech_voice_not_downloaded
                    ),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (managing) {
                if (isDownloaded) {
                    TextButton(onClick = { onPreview(name) }, enabled = enabled) {
                        Text(tr(R.string.speech_voice_preview))
                    }
                    if (name != SupertonicModels.DEFAULT_VOICE)
                        TextButton(onClick = { onDelete(name) }) {
                            Text(tr(R.string.speech_voice_delete))
                        }
                } else {
                    TextButton(onClick = { onDownload(name) }) {
                        Text(tr(R.string.speech_voice_download))
                    }
                }
            }
        }
    }
}

@Composable
private fun LanguageSection(
    p: SpeechPreferences,
    languages: List<String>,
    onSpeech: (SpeechPreferences) -> Unit,
) {
    SectionHeader(tr(R.string.speech_language))
    val configuration = LocalConfiguration.current
    val deviceCode =
        SupertonicLanguages.fromLocale(LocalLocale.current.platformLocale)
    var expanded by remember { mutableStateOf(false) }

    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp)
            .clickable { onSpeech(p.copy(languageMode = LanguageMode.AUTO)) },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(
            p.languageMode == LanguageMode.AUTO,
            { onSpeech(p.copy(languageMode = LanguageMode.AUTO)) },
        )
        Column(Modifier.weight(1f)) {
            Text(tr(R.string.speech_language_auto))
            Text(
                tr(R.string.speech_language_auto_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable {
            onSpeech(p.copy(languageMode = LanguageMode.MANUAL, language = deviceCode))
        },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(
            p.languageMode == LanguageMode.MANUAL && p.language == deviceCode,
            { onSpeech(p.copy(languageMode = LanguageMode.MANUAL, language = deviceCode)) },
        )
        Text("${tr(R.string.speech_language_device)} · ${deviceCode.uppercase()}")
    }
    Box {
        OutlinedButton(onClick = { expanded = true }) {
            Text(
                if (p.languageMode == LanguageMode.MANUAL) p.language.uppercase()
                else tr(R.string.speech_language)
            )
        }
        DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            for (code in languages) {
                DropdownMenuItem(
                    text = { Text(if (code == SupertonicLanguages.UNKNOWN) "$code (fallback)" else code.uppercase()) },
                    onClick = {
                        onSpeech(p.copy(languageMode = LanguageMode.MANUAL, language = code))
                        expanded = false
                    },
                )
            }
        }
    }
}

@Composable
private fun QualitySection(p: SpeechPreferences, onSpeech: (SpeechPreferences) -> Unit) {
    SectionHeader(tr(R.string.speech_quality))
    for (preset in QualityPreset.entries) {
        val label =
            tr(
                when (preset) {
                    QualityPreset.FAST -> R.string.speech_quality_fast
                    QualityPreset.BALANCED -> R.string.speech_quality_balanced
                    QualityPreset.HIGH -> R.string.speech_quality_high
                }
            )
        Row(
            Modifier.fillMaxWidth().heightIn(min = 52.dp).clickable {
                onSpeech(p.copy(qualityPreset = preset, customSteps = null))
            },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(
                p.qualityPreset == preset && p.customSteps == null,
                { onSpeech(p.copy(qualityPreset = preset, customSteps = null)) },
            )
            Text(label)
        }
    }
}

@Composable
private fun SpeedSection(p: SpeechPreferences, onSpeech: (SpeechPreferences) -> Unit) {
    SectionHeader(tr(R.string.speech_speed))
    var local by remember(p.speed) { mutableFloatStateOf(p.speed) }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Slider(
            value = local,
            onValueChange = { local = it },
            onValueChangeFinished = { onSpeech(p.copy(speed = local)) },
            valueRange = SupertonicQuality.MIN_SPEED..SupertonicQuality.MAX_SPEED,
            modifier = Modifier.weight(1f),
        )
        Text(
            tr(R.string.speech_speed_value, "%.2f".format(Locale.US, local.toDouble())),
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun AdvancedSection(
    expanded: Boolean,
    onToggle: () -> Unit,
    prefs: SpeechPreferences,
    hasDiagnosticAudio: Boolean,
    onUpdate: ((SpeechPreferences) -> SpeechPreferences) -> Unit,
    onShare: () -> Unit,
    onClearDiagnostics: () -> Unit,
) {
    SectionHeader(tr(R.string.speech_advanced)) {
        IconButton(onClick = onToggle) {
            Icon(
                if (expanded) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore,
                contentDescription = tr(R.string.speech_advanced),
            )
        }
    }
    if (!expanded) {
        Text(
            tr(R.string.speech_advanced_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        return
    }

    var steps by rememberSaveable(prefs.customSteps) { mutableStateOf(prefs.customSteps?.toString().orEmpty()) }
    OutlinedTextField(
        value = steps,
        onValueChange = { steps = it.filter(Char::isDigit).take(2) },
        modifier = Modifier.fillMaxWidth(),
        label = { Text(tr(R.string.speech_steps)) },
        supportingText = {
            Text(
                tr(
                    R.string.speech_steps_hint,
                    SupertonicQuality.STEPS_RANGE.first,
                    SupertonicQuality.STEPS_RANGE.last,
                )
            )
        },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        singleLine = true,
        trailingIcon = {
            if (prefs.customSteps != null)
                TextButton(onClick = { onUpdate { it.copy(customSteps = null) } }) {
                    Text(tr(R.string.speech_steps_use_preset))
                }
        },
    )
    if (steps != prefs.customSteps?.toString() && steps.isNotBlank()) {
        val parsed = steps.toIntOrNull()
        LaunchedEffect(steps) {
            delay(400)
            if (parsed != null && parsed in SupertonicQuality.STEPS_RANGE) {
                onUpdate { it.copy(customSteps = parsed) }
            }
        }
    }

    var chunkLength by rememberSaveable(prefs.chunkLength) {
        mutableStateOf(prefs.chunkLength?.toString().orEmpty())
    }
    OutlinedTextField(
        value = chunkLength,
        onValueChange = { chunkLength = it.filter(Char::isDigit).take(4) },
        modifier = Modifier.fillMaxWidth(),
        label = { Text(tr(R.string.speech_chunk_length)) },
        supportingText = { Text(tr(R.string.speech_chunk_length_hint)) },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        singleLine = true,
    )
    if (chunkLength != prefs.chunkLength?.toString()) {
        LaunchedEffect(chunkLength) {
            delay(400)
            val parsed = chunkLength.toIntOrNull()
            onUpdate {
                it.copy(
                    chunkLength = parsed?.takeIf { v -> v in SupertonicQuality.CHUNK_LENGTH_RANGE }
                )
            }
        }
    }

    var silence by rememberSaveable(prefs.chunkSilenceMs) {
        mutableStateOf(prefs.chunkSilenceMs.toString())
    }
    OutlinedTextField(
        value = silence,
        onValueChange = { silence = it.filter(Char::isDigit).take(4) },
        modifier = Modifier.fillMaxWidth(),
        label = { Text(tr(R.string.speech_chunk_silence)) },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        singleLine = true,
    )
    if (silence != prefs.chunkSilenceMs.toString()) {
        LaunchedEffect(silence) {
            delay(400)
            silence.toIntOrNull()?.let { value -> onUpdate { it.copy(chunkSilenceMs = value) } }
        }
    }

    var maxChars by rememberSaveable(prefs.maxSpeechChars) {
        mutableStateOf(prefs.maxSpeechChars.toString())
    }
    OutlinedTextField(
        value = maxChars,
        onValueChange = { maxChars = it.filter(Char::isDigit).take(5) },
        modifier = Modifier.fillMaxWidth(),
        label = { Text(tr(R.string.speech_max_chars)) },
        supportingText = {
            Text(
                if (maxChars.toIntOrNull() == 0) tr(R.string.speech_max_chars_unlimited)
                else tr(R.string.speech_max_chars_hint)
            )
        },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        singleLine = true,
    )
    if (maxChars != prefs.maxSpeechChars.toString()) {
        LaunchedEffect(maxChars) {
            delay(400)
            onUpdate { it.copy(maxSpeechChars = (maxChars.toIntOrNull() ?: 0).coerceAtLeast(0)) }
        }
    }

    SettingSwitch(
        tr(R.string.speech_diagnostic_logging),
        prefs.diagnosticLogging,
        { onUpdate { p -> p.copy(diagnosticLogging = it) } },
    )
    SettingSwitch(
        tr(R.string.speech_export_wav),
        prefs.exportWav,
        { onUpdate { p -> p.copy(exportWav = it) } },
        tr(R.string.speech_export_wav_hint),
    )
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedButton(onClick = onShare, enabled = hasDiagnosticAudio) {
            Text(tr(R.string.speech_export_share))
        }
        TextButton(onClick = onClearDiagnostics, enabled = hasDiagnosticAudio) {
            Text(tr(R.string.speech_clear_diagnostics))
        }
    }
    if (!hasDiagnosticAudio)
        Text(
            tr(R.string.speech_export_empty),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
}
