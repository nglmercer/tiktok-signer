package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.RecyclerView

/** Renders feed rooms; a tap selects the room for connecting. */
class FeedAdapter(
    private val onSelect: (Feed.LiveRoom) -> Unit,
) : RecyclerView.Adapter<FeedAdapter.ViewHolder>() {

    private var rooms: List<Feed.LiveRoom> = emptyList()

    class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        val roomLine: TextView = view.findViewById(R.id.roomLine)
        val titleLine: TextView = view.findViewById(R.id.titleLine)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_feed, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val room = rooms[position]
        val name = room.nickname.ifEmpty { room.uniqueId }
        holder.roomLine.text = "$name (@${room.uniqueId}) · ${room.viewers} watching"
        holder.titleLine.text = room.title.ifEmpty { "(no title)" }
        holder.itemView.setOnClickListener { onSelect(room) }
    }

    override fun getItemCount(): Int = rooms.size

    fun submitList(rooms: List<Feed.LiveRoom>) {
        this.rooms = rooms
        notifyDataSetChanged()
    }
}
