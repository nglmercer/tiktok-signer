package com.example.ttlsigner

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.data.StudioDb

/**
 * Points leaderboard rows: rank, handle, balance, level. The currency name
 * comes from [PointsConfig] and is bound per row via [currency].
 */
class PointsAdapter(
    private var currency: String = "coins",
) : ListAdapter<StudioDb.Viewer, PointsAdapter.ViewHolder>(DIFF) {

    companion object {
        private val DIFF = object : DiffUtil.ItemCallback<StudioDb.Viewer>() {
            override fun areItemsTheSame(a: StudioDb.Viewer, b: StudioDb.Viewer): Boolean =
                a.uniqueId == b.uniqueId
            override fun areContentsTheSame(a: StudioDb.Viewer, b: StudioDb.Viewer): Boolean = a == b
        }

        /** `1500.0` → "1500", `12.5` → "12.5". */
        fun formatPoints(value: Double): String =
            if (value == value.toLong().toDouble()) value.toLong().toString() else value.toString()
    }

    fun setCurrency(name: String) {
        if (currency != name) {
            currency = name
            notifyDataSetChanged()
        }
    }

    class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        val rank: TextView = view.findViewById(R.id.rankText)
        val user: TextView = view.findViewById(R.id.userText)
        val balance: TextView = view.findViewById(R.id.balanceText)
        val level: TextView = view.findViewById(R.id.levelText)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_points, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val viewer = getItem(position)
        holder.rank.text = "#${position + 1}"
        holder.user.text = "@${viewer.uniqueId}"
        holder.balance.text = "${formatPoints(viewer.points)} $currency"
        holder.level.text = "Lv ${viewer.level}"
    }
}
