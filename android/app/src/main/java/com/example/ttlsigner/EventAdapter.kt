package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.ImageView
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.events.EventDisplayConfig
import com.example.ttlsigner.events.LiveEvent
import com.example.ttlsigner.ui.ImageLoader
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * The event reader's rows: a category badge, the rendered line (see
 * [EventFormat]), and the arrival time. Backed by [ListAdapter] so filter
 * changes animate instead of flashing. [setDisplay] restyles every row from
 * the reader's [EventDisplayConfig] — minimalist by default.
 */
class EventAdapter(
    private var display: EventDisplayConfig = EventDisplayConfig(),
) : ListAdapter<LiveEvent, EventAdapter.ViewHolder>(DIFF) {

    fun setDisplay(next: EventDisplayConfig) {
        if (display != next) {
            display = next
            notifyDataSetChanged()
        }
    }

    companion object {
        private val CLOCK = SimpleDateFormat("HH:mm:ss", Locale.US)

        private val DIFF = object : DiffUtil.ItemCallback<LiveEvent>() {
            override fun areItemsTheSame(a: LiveEvent, b: LiveEvent): Boolean =
                a.at == b.at && a.raw == b.raw
            override fun areContentsTheSame(a: LiveEvent, b: LiveEvent): Boolean = a == b
        }
    }

    class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        val avatar: ImageView = view.findViewById(R.id.eventAvatar)
        val badge: TextView = view.findViewById(R.id.eventBadge)
        val body: TextView = view.findViewById(R.id.eventBody)
        val giftImage: ImageView = view.findViewById(R.id.eventGiftImage)
        val time: TextView = view.findViewById(R.id.eventTime)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_event, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val event = getItem(position)
        // Avatar only when the sender shipped one: userless rows (room stats,
        // unknowns) keep the clean text-only shape.
        if (event.avatarUrl.isEmpty()) {
            holder.avatar.visibility = View.GONE
        } else {
            holder.avatar.visibility = View.VISIBLE
            ImageLoader.load(
                holder.avatar,
                event.avatarUrl,
                placeholder = R.drawable.ic_ev_member,
                fallback = R.drawable.ic_ev_member,
            )
        }
        val giftUrl = if (event.category == LiveEvent.Category.GIFT) event.giftImageUrl else ""
        if (giftUrl.isEmpty()) {
            holder.giftImage.visibility = View.GONE
        } else {
            holder.giftImage.visibility = View.VISIBLE
            ImageLoader.load(
                holder.giftImage,
                giftUrl,
                placeholder = R.drawable.ic_ev_gift,
                fallback = R.drawable.ic_ev_gift,
            )
        }
        holder.badge.visibility = if (display.showBadge) View.VISIBLE else View.GONE
        holder.badge.text = event.category.name.lowercase()
        holder.body.text = EventFormat.line(event.raw.ifEmpty { "{}" })
        holder.body.maxLines = if (display.singleLine) 1 else 3
        holder.time.visibility = if (display.showTime) View.VISIBLE else View.GONE
        holder.time.text = CLOCK.format(Date(event.at))
        val density = holder.itemView.resources.displayMetrics.density
        val vertical = ((if (display.compact) 4 else 8) * density).toInt()
        holder.itemView.setPadding(
            holder.itemView.paddingStart,
            vertical,
            holder.itemView.paddingEnd,
            vertical,
        )
    }
}
