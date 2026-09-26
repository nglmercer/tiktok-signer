package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.events.LiveEvent
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * The event reader's rows: a category badge, the rendered line (see
 * [EventFormat]), and the arrival time. Backed by [ListAdapter] so filter
 * changes animate instead of flashing.
 */
class EventAdapter : ListAdapter<LiveEvent, EventAdapter.ViewHolder>(DIFF) {

    companion object {
        private val CLOCK = SimpleDateFormat("HH:mm:ss", Locale.US)

        private val DIFF = object : DiffUtil.ItemCallback<LiveEvent>() {
            override fun areItemsTheSame(a: LiveEvent, b: LiveEvent): Boolean =
                a.at == b.at && a.raw == b.raw
            override fun areContentsTheSame(a: LiveEvent, b: LiveEvent): Boolean = a == b
        }
    }

    class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        val badge: TextView = view.findViewById(R.id.eventBadge)
        val body: TextView = view.findViewById(R.id.eventBody)
        val time: TextView = view.findViewById(R.id.eventTime)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_event, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val event = getItem(position)
        holder.badge.text = event.category.name.lowercase()
        holder.body.text = EventFormat.line(event.raw.ifEmpty { "{}" })
        holder.time.text = CLOCK.format(Date(event.at))
    }
}
