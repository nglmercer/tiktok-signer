package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.RecyclerView

/** Scrolling event tail. Capped so a busy room cannot grow memory without bound. */
class EventAdapter : RecyclerView.Adapter<EventAdapter.ViewHolder>() {

    companion object {
        const val MAX_LINES = 300
    }

    private val lines = ArrayDeque<String>()

    class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        val text: TextView = view as TextView
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_event, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        holder.text.text = lines[position]
    }

    override fun getItemCount(): Int = lines.size

    /** Append a rendered line, dropping the oldest past the cap. */
    fun append(line: String) {
        lines.addLast(line)
        if (lines.size > MAX_LINES) lines.removeFirst()
        notifyDataSetChanged()
    }

    fun clear() {
        lines.clear()
        notifyDataSetChanged()
    }

    /** Replace the whole tail; the view model owns the cap. */
    fun submitList(rendered: List<String>) {
        lines.clear()
        lines.addAll(rendered.takeLast(MAX_LINES))
        notifyDataSetChanged()
    }
}
