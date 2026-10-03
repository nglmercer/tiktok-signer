package dev.nglmercer.tiktools.feature.rewards

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.*
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nglmercer.tiktools.data.rewards.PointsConfig
import dev.nglmercer.tiktools.ui.components.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RewardsScreen(vm: RewardsViewModel) {
    val viewers = vm.leaderboard.collectAsStateWithLifecycle().value
    val config = vm.config.collectAsStateWithLifecycle().value
    val error = vm.error.collectAsStateWithLifecycle().value
    var rates by rememberSaveable { mutableStateOf(false) }
    var selected by rememberSaveable { mutableStateOf<String?>(null) }
    var reset by remember { mutableStateOf(false) }
    if (rates) {
        RewardRates(
            config,
            error,
            onSave = { vm.saveRates(it) { rates = false } },
            onBack = { rates = false },
        )
        return
    }
    LazyColumn(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
        item {
            SectionHeader("Leaderboard") {
                Row {
                    IconButton(onClick = { rates = true }) {
                        Icon(Icons.Outlined.Tune, "Reward rates")
                    }
                    IconButton(onClick = { reset = true }) {
                        Icon(Icons.Outlined.RestartAlt, "Reset all balances")
                    }
                }
            }
            ErrorBanner(error, vm::dismissError)
        }
        if (viewers.isEmpty())
            item {
                EmptyState(
                    "No rewards yet",
                    "Viewer balances appear as LIVE events arrive",
                    Icons.Outlined.EmojiEvents,
                )
            }
        itemsIndexed(viewers, key = { _, user -> user.uniqueId }) { index, user ->
            Row(
                Modifier.fillMaxWidth()
                    .heightIn(min = 80.dp)
                    .clickable { selected = user.uniqueId }
                    .padding(vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text("#${index+1}", style = MaterialTheme.typography.labelLarge)
                CreatorAvatar("", user.nickname.ifEmpty { user.uniqueId })
                Column(Modifier.weight(1f)) {
                    Text(
                        user.nickname.ifEmpty { "@${user.uniqueId}" },
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        "Level ${user.level} · @${user.uniqueId}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(
                    "${amount(user.points)} ${config.currencyName}",
                    style = MaterialTheme.typography.labelLarge,
                )
            }
            HorizontalDivider()
        }
    }
    selected?.let { id ->
        val viewer = viewers.firstOrNull { it.uniqueId == id }
        val earnings =
            remember(id) { vm.earnings(id) }
                .collectAsStateWithLifecycle(initialValue = emptyList())
                .value
        var delta by rememberSaveable(id) { mutableStateOf("") }
        ModalBottomSheet(onDismissRequest = { selected = null }) {
            Column(
                Modifier.verticalScroll(rememberScrollState())
                    .padding(16.dp)
                    .imePadding()
                    .navigationBarsPadding(),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    viewer?.nickname?.ifEmpty { "@$id" } ?: "@$id",
                    style = MaterialTheme.typography.headlineMedium,
                )
                Text(
                    "${amount(viewer?.points ?: 0.0)} ${config.currencyName} · Level ${viewer?.level ?: 1}"
                )
                SectionHeader("Earned from")
                for (earning in earnings) SettingItem(
                    earning.category.lowercase().replaceFirstChar(Char::uppercase),
                    amount(earning.amount),
                )
                val untracked = (viewer?.points ?: 0.0) - earnings.sumOf { it.amount }
                if (untracked > 0.001) SettingItem("Previous balance", amount(untracked))
                ErrorBanner(error, vm::dismissError)
                OutlinedTextField(
                    delta,
                    { delta = it },
                    label = { Text("Adjustment (negative to subtract)") },
                    keyboardOptions =
                        androidx.compose.foundation.text.KeyboardOptions(
                            keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal
                        ),
                    singleLine = true,
                )
                Button(
                    onClick = {
                        vm.adjust(id, delta.toDouble())
                        delta = ""
                    },
                    enabled = delta.toDoubleOrNull()?.isFinite() == true,
                ) {
                    Text("Adjust points")
                }
            }
        }
    }
    if (reset)
        AlertDialog(
            onDismissRequest = { reset = false },
            title = { Text("Reset all balances?") },
            text = {
                Text("Viewers stay in the leaderboard with zero points. This cannot be undone.")
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        vm.reset()
                        reset = false
                    }
                ) {
                    Text("Reset")
                }
            },
            dismissButton = { TextButton(onClick = { reset = false }) { Text("Cancel") } },
        )
}

@Composable
private fun RewardRates(
    config: PointsConfig,
    error: String?,
    onSave: (PointsConfig) -> Unit,
    onBack: () -> Unit,
) {
    var currency by rememberSaveable { mutableStateOf(config.currencyName) }
    var level by rememberSaveable { mutableStateOf(config.pointsPerLevel.toString()) }
    var chat by rememberSaveable { mutableStateOf(config.pointsPerChat.toString()) }
    var chatOn by rememberSaveable { mutableStateOf(config.pointsPerChatEnabled) }
    var gift by rememberSaveable { mutableStateOf(config.pointsPerCoin.toString()) }
    var giftOn by rememberSaveable { mutableStateOf(config.pointsPerCoinEnabled) }
    var like by rememberSaveable { mutableStateOf(config.pointsPerLike.toString()) }
    var likeOn by rememberSaveable { mutableStateOf(config.pointsPerLikeEnabled) }
    var follow by rememberSaveable { mutableStateOf(config.pointsPerFollow.toString()) }
    var followOn by rememberSaveable { mutableStateOf(config.pointsPerFollowEnabled) }
    var share by rememberSaveable { mutableStateOf(config.pointsPerShare.toString()) }
    var shareOn by rememberSaveable { mutableStateOf(config.pointsPerShareEnabled) }
    var join by rememberSaveable { mutableStateOf(config.pointsPerJoin.toString()) }
    var joinOn by rememberSaveable { mutableStateOf(config.pointsPerJoinEnabled) }
    var expanded by rememberSaveable { mutableStateOf("Chat") }
    val valid =
        listOf(chat, gift, like, follow, share, join).all {
            it.toDoubleOrNull()?.let { n ->
                n.isFinite() && n >= 0 && n <= PointsConfig.MAX_RATE
            } == true
        } && level.toDoubleOrNull()?.let { it.isFinite() && it >= PointsConfig.MIN_LEVEL } == true
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            TextButton(onClick = onBack) {
                Icon(Icons.Outlined.ArrowBack, null)
                Text("Leaderboard")
            }
            Text("Reward rates", style = MaterialTheme.typography.headlineMedium)
            ErrorBanner(error)
        }
        item {
            OutlinedTextField(
                currency,
                { currency = it },
                label = { Text("Currency name") },
                singleLine = true,
            )
            OutlinedTextField(
                level,
                { level = it },
                label = { Text("Points per level (minimum 10)") },
                singleLine = true,
            )
        }
        fun LazyListScope.rate(
            title: String,
            value: String,
            onValue: (String) -> Unit,
            enabled: Boolean,
            onEnabled: (Boolean) -> Unit,
            label: String,
        ) {
            item {
                SectionHeader(title) {
                    TextButton(onClick = { expanded = if (expanded == title) "" else title }) {
                        Text(if (expanded == title) "Close" else "Edit")
                    }
                }
                SettingSwitch("Enabled", enabled, onEnabled)
                if (expanded == title)
                    OutlinedTextField(
                        value,
                        onValue,
                        label = { Text(label) },
                        keyboardOptions =
                            androidx.compose.foundation.text.KeyboardOptions(
                                keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal
                            ),
                        singleLine = true,
                    )
                else
                    Text(
                        "$value · $label",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                HorizontalDivider(Modifier.padding(top = 8.dp))
            }
        }
        rate("Chat", chat, { chat = it }, chatOn, { chatOn = it }, "Per message")
        rate("Gift", gift, { gift = it }, giftOn, { giftOn = it }, "Per diamond")
        rate("Like", like, { like = it }, likeOn, { likeOn = it }, "Per like")
        rate("Follow", follow, { follow = it }, followOn, { followOn = it }, "Per follow")
        rate("Share", share, { share = it }, shareOn, { shareOn = it }, "Per share")
        rate("Join", join, { join = it }, joinOn, { joinOn = it }, "Per join")
        item {
            Button(
                onClick = {
                    onSave(
                        PointsConfig(
                            currency,
                            chat.toDouble(),
                            chatOn,
                            gift.toDouble(),
                            giftOn,
                            like.toDouble(),
                            likeOn,
                            follow.toDouble(),
                            followOn,
                            share.toDouble(),
                            shareOn,
                            join.toDouble(),
                            joinOn,
                            level.toDouble(),
                        )
                    )
                },
                enabled = valid && currency.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Save rates")
            }
        }
    }
}
