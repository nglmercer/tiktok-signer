package dev.nglmercer.tiktools.ui.components

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.*
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalLocale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import dev.nglmercer.tiktools.R
import dev.nglmercer.tiktools.core.model.LiveEmote
import dev.nglmercer.tiktools.core.model.LiveEvent
import dev.nglmercer.tiktools.live.LiveSessionState
import dev.nglmercer.tiktools.live.*
import dev.nglmercer.tiktools.ui.theme.LocalSemanticColors
import java.util.Locale

@Composable
fun tr(id: Int, vararg formatArgs: Any): String = stringResource(id, *formatArgs)

@Composable
fun trPlural(id: Int, quantity: Int, vararg formatArgs: Any): String =
    pluralStringResource(id, quantity, *formatArgs)

@Composable
private fun appLocale(): Locale =
    LocalLocale.current.platformLocale

@Composable
fun localizedCompact(value: Long): String = compact(value, appLocale())

@Composable
fun localizedAmount(value: Double): String = amount(value, appLocale())

@Composable
fun localizedEventTime(at: Long): String = eventTime(at, appLocale())

@Composable
fun LiveEvent.Category.localizedName(): String =
    tr(
        when (this) {
            LiveEvent.Category.CHAT -> R.string.category_chat
            LiveEvent.Category.EMOTE -> R.string.category_emote
            LiveEvent.Category.GIFT -> R.string.category_gift
            LiveEvent.Category.LIKE -> R.string.category_like
            LiveEvent.Category.FOLLOW -> R.string.category_follow
            LiveEvent.Category.SHARE -> R.string.category_share
            LiveEvent.Category.SOCIAL -> R.string.category_social
            LiveEvent.Category.JOIN -> R.string.category_join
            LiveEvent.Category.MEMBER -> R.string.category_member
            LiveEvent.Category.ROOM -> R.string.category_room
            LiveEvent.Category.UNKNOWN -> R.string.category_unknown
        }
    )

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
                IconButton(onClick = onDismiss) {
                    Icon(Icons.Outlined.Close, stringResource(R.string.dismiss_error))
                }
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

/**
 * One emote thumbnail.
 *
 * Loading is async and cached by Coil keyed on the URL, so the same emote across many rows is
 * downloaded once and never re-fetched on recomposition. A placeholder is shown while the image
 * loads and an error/fallback icon replaces a broken URL, so a dead CDN link never blanks the row.
 * The `remember`ed request keeps recomposition from rebuilding the request object.
 */
@Composable
fun EmoteImage(url: String, contentDescription: String?, size: Dp = 28.dp) {
    val context = LocalContext.current
    val request = remember(url) {
        ImageRequest.Builder(context).data(url).crossfade(true).build()
    }
    val placeholder = rememberVectorPainter(Icons.Outlined.EmojiEmotions)
    Box(
        Modifier.size(size)
            .clip(RoundedCornerShape(6.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        AsyncImage(
            model = request,
            contentDescription = contentDescription,
            placeholder = placeholder,
            error = placeholder,
            fallback = placeholder,
            modifier = Modifier.fillMaxSize().padding(2.dp),
            contentScale = ContentScale.Fit,
        )
    }
}

/**
 * A bounded row of emote artwork for a chat comment or a standalone emote event.
 *
 * Only emotes that actually carry artwork are drawn; the rest are represented by the surrounding
 * text/count. The list is capped so a hostile or exceptionally long message cannot inflate the
 * composition, and every item has a stable URL key so scrolling and recomposition reuse the cache.
 */
@Composable
fun EmoteStrip(emotes: List<LiveEmote>, size: Dp = 28.dp, max: Int = 8) {
    val shown = ArrayList<LiveEmote>(minOf(emotes.size, max))
    for (emote in emotes) {
        if (shown.size >= max) break
        if (!emote.imageUrl.isNullOrBlank()) shown.add(emote)
    }
    if (shown.isEmpty()) return
    Row(
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (emote in shown) {
            EmoteImage(emote.imageUrl!!, emote.shortcode ?: emote.id, size)
        }
    }
}

@Composable
fun LiveStatusBadge(state: LiveSessionState) {
    val semantic = LocalSemanticColors.current
    val context = LocalContext.current
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
        Text(
            state.localizedLabel(context),
            color = color,
            style = MaterialTheme.typography.labelLarge,
        )
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

fun compact(value: Long, locale: Locale = Locale.getDefault()): String =
    when {
        value >= 1_000_000 -> String.format(locale, "%.1fM", value / 1_000_000.0)
        value >= 1000 -> String.format(locale, "%.1fk", value / 1000.0)
        else -> value.toString()
    }

fun amount(value: Double, locale: Locale = Locale.getDefault()): String =
    String.format(locale, "%.1f", value)
