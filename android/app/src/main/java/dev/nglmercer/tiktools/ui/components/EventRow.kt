package dev.nglmercer.tiktools.ui.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.*
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import dev.nglmercer.tiktools.core.model.*
import java.text.SimpleDateFormat
import java.util.*

fun LiveEvent.Category.icon(): ImageVector =
    when (this) {
        LiveEvent.Category.CHAT -> Icons.Outlined.ChatBubbleOutline
        LiveEvent.Category.GIFT -> Icons.Outlined.CardGiftcard
        LiveEvent.Category.LIKE -> Icons.Outlined.FavoriteBorder
        LiveEvent.Category.FOLLOW -> Icons.Outlined.PersonAddAlt
        LiveEvent.Category.SHARE -> Icons.Outlined.Share
        LiveEvent.Category.JOIN,
        LiveEvent.Category.MEMBER -> Icons.Outlined.PeopleOutline
        LiveEvent.Category.ROOM -> Icons.Outlined.Visibility
        LiveEvent.Category.UNKNOWN -> Icons.Outlined.HelpOutline
    }

fun LiveEvent.summary(): String =
    when (category) {
        LiveEvent.Category.CHAT -> text
        LiveEvent.Category.GIFT ->
            "${text.ifEmpty { "Gift" }} ×$count · ${diamonds*count.coerceAtLeast(1)} diamonds"
        LiveEvent.Category.LIKE -> "$count likes"
        LiveEvent.Category.FOLLOW -> "Followed"
        LiveEvent.Category.SHARE -> "Shared the LIVE"
        LiveEvent.Category.JOIN -> "Joined"
        LiveEvent.Category.MEMBER -> "Member action $count"
        LiveEvent.Category.ROOM -> "${compact(count)} viewers"
        LiveEvent.Category.UNKNOWN -> text.ifEmpty { "Unknown event" }
    }

fun eventTime(at: Long): String = SimpleDateFormat("HH:mm:ss", Locale.getDefault()).format(Date(at))

@Composable
fun EventRow(
    event: LiveEvent,
    display: EventDisplayConfig = EventDisplayConfig(),
    onClick: () -> Unit = {},
) {
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = 64.dp)
            .clickable(onClick = onClick)
            .padding(vertical = if (display.compact) 8.dp else 16.dp)
            .semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        CreatorAvatar(
            event.avatarUrl,
            event.nickname.ifEmpty { event.user },
            if (display.compact) 36.dp else 44.dp,
        )
        Column(Modifier.weight(1f)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                if (display.showBadge)
                    Icon(
                        event.category.icon(),
                        null,
                        Modifier.size(16.dp),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                Text(
                    event.nickname.ifEmpty { event.user.ifEmpty { "Room" } },
                    style = MaterialTheme.typography.labelLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Text(
                event.summary(),
                maxLines = if (display.singleLine) 1 else 4,
                overflow = TextOverflow.Ellipsis,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        if (event.giftImageUrl.isNotEmpty())
            AsyncImage(event.giftImageUrl, null, Modifier.size(32.dp))
        if (display.showTime)
            Text(
                eventTime(event.at),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
    }
}
