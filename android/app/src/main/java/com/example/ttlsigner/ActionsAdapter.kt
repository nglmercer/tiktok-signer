package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.actions.EventAction
import com.google.android.material.switchmaterial.SwitchMaterial

/**
 * Fetch-action rows: name, trigger summary, method + URL, enable switch, and
 * edit / test / delete buttons. All taps delegate to [Listener]; the fragment
 * (via the view model) owns every mutation.
 */
class ActionsAdapter(private val listener: Listener) :
    ListAdapter<EventAction, ActionsAdapter.ViewHolder>(DIFF) {

    interface Listener {
        fun onToggle(action: EventAction, enabled: Boolean)
        fun onEdit(action: EventAction)
        fun onTest(action: EventAction)
        fun onDelete(action: EventAction)
    }

    companion object {
        private val DIFF = object : DiffUtil.ItemCallback<EventAction>() {
            override fun areItemsTheSame(a: EventAction, b: EventAction): Boolean = a.id == b.id
            override fun areContentsTheSame(a: EventAction, b: EventAction): Boolean = a == b
        }
    }

    class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        val name: TextView = view.findViewById(R.id.actionName)
        val detail: TextView = view.findViewById(R.id.actionDetail)
        val toggle: SwitchMaterial = view.findViewById(R.id.actionToggle)
        val edit: Button = view.findViewById(R.id.actionEdit)
        val test: Button = view.findViewById(R.id.actionTest)
        val delete: Button = view.findViewById(R.id.actionDelete)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_action, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val action = getItem(position)
        holder.name.text = action.name.ifEmpty { "Unnamed action" }
        val cooldown = if (action.cooldownSecs > 0) " · cooldown ${action.cooldownSecs}s" else ""
        holder.detail.text = "${action.triggerSummary()} · ${action.method} ${action.url}$cooldown"
        holder.toggle.setOnCheckedChangeListener(null)
        holder.toggle.isChecked = action.enabled
        holder.toggle.setOnCheckedChangeListener { _, checked ->
            listener.onToggle(action, checked)
        }
        holder.edit.setOnClickListener { listener.onEdit(action) }
        holder.test.setOnClickListener { listener.onTest(action) }
        holder.delete.setOnClickListener { listener.onDelete(action) }
    }
}
