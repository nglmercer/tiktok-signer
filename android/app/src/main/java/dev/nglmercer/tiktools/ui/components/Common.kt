package dev.nglmercer.tiktools.ui.components

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.*
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import dev.nglmercer.tiktools.live.*
import dev.nglmercer.tiktools.ui.theme.LocalSemanticColors
import java.util.Locale

@Composable
fun SectionHeader(title: String, action: @Composable (() -> Unit)? = null) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
        action?.invoke()
    }
}

@Composable
fun EmptyState(title: String, detail: String = "", icon: ImageVector = Icons.Outlined.Inbox) {
    Column(
        Modifier.fillMaxWidth().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(icon, null, Modifier.size(32.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(title, style = MaterialTheme.typography.titleMedium)
        if (detail.isNotBlank())
            Text(
                detail,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
    }
}

@Composable
fun ErrorBanner(message: String?, onDismiss: (() -> Unit)? = null) {
    if (message == null) return
    Surface(color = MaterialTheme.colorScheme.errorContainer, shape = MaterialTheme.shapes.medium) {
        Row(
            Modifier.fillMaxWidth().padding(12.dp).semantics { liveRegion = LiveRegionMode.Polite },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(message, Modifier.weight(1f), color = MaterialTheme.colorScheme.onErrorContainer)
            if (onDismiss != null)
                IconButton(onClick = onDismiss) { Icon(Icons.Outlined.Close, "Dismiss error") }
        }
    }
}

@Composable
fun CreatorAvatar(url: String, name: String, size: androidx.compose.ui.unit.Dp = 44.dp) {
    Box(
        Modifier.size(size)
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.secondaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        if (url.isNotBlank())
            AsyncImage(
                url,
                null,
                Modifier.fillMaxSize(),
                contentScale = androidx.compose.ui.layout.ContentScale.Crop,
            )
        else
            Text(
                name.take(1).uppercase().ifEmpty { "?" },
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
    }
}

@Composable
fun LiveStatusBadge(state: LiveSessionState) {
    val semantic = LocalSemanticColors.current
    val color =
        when (state) {
            is LiveSessionState.Connected -> semantic.live
            is LiveSessionState.Failed -> semantic.error
            LiveSessionState.Disconnected -> MaterialTheme.colorScheme.onSurfaceVariant
            else -> semantic.warning
        }
    Row(
        Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(color))
        Text(state.label(), color = color, style = MaterialTheme.typography.labelLarge)
    }
}

@Composable
fun StatItem(value: String, label: String, modifier: Modifier = Modifier) {
    Column(modifier.padding(vertical = 12.dp).semantics(mergeDescendants = true) {}) {
        Text(value, style = MaterialTheme.typography.headlineSmall)
        Text(
            label,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
fun SettingItem(title: String, detail: String, onClick: (() -> Unit)? = null) {
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = 64.dp)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title)
            Text(
                detail,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        if (onClick != null) Icon(Icons.Outlined.ChevronRight, null)
    }
}

@Composable
fun SettingSwitch(title: String, value: Boolean, onChange: (Boolean) -> Unit, detail: String = "") {
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = 64.dp)
            .toggleable(value = value, role = Role.Switch, onValueChange = onChange),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(title)
            if (detail.isNotEmpty())
                Text(
                    detail,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
        }
        Switch(value, onCheckedChange = null)
    }
}

fun compact(value: Long): String =
    when {
        value >= 1_000_000 -> String.format(Locale.US, "%.1fM", value / 1_000_000.0)
        value >= 1000 -> String.format(Locale.US, "%.1fk", value / 1000.0)
        else -> value.toString()
    }

fun amount(value: Double): String = String.format(Locale.US, "%.1f", value)
