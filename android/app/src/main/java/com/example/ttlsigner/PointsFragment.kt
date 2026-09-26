package com.example.ttlsigner

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.CheckBox
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.fragment.app.Fragment
import androidx.fragment.app.activityViewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.points.PointsConfig
import com.example.ttlsigner.ui.CollapsibleCard
import com.example.ttlsigner.ui.EmptyStateView
import com.example.ttlsigner.ui.UiKit.addMinimalDividers
import com.google.android.material.textfield.TextInputEditText
import kotlinx.coroutines.launch

/**
 * Points tab: the SQLite-backed leaderboard, the per-event rates editor, a
 * manual adjust box, and reset. Rates mirror TikTools desktop's
 * `PointsConfig` (rate + enable flag per trigger).
 */
class PointsFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()

    /** One editable rate row: getter/setter into a [PointsConfig] copy. */
    private data class RateRow(
        val label: String,
        val getRate: (PointsConfig) -> Double,
        val setRate: (PointsConfig, Double) -> PointsConfig,
        val getEnabled: (PointsConfig) -> Boolean,
        val setEnabled: (PointsConfig, Boolean) -> PointsConfig,
    )

    private val rows = listOf(
        RateRow("Chat", { it.pointsPerChat }, { c, v -> c.copy(pointsPerChat = v) },
            { it.pointsPerChatEnabled }, { c, v -> c.copy(pointsPerChatEnabled = v) }),
        RateRow("Gift (per diamond)", { it.pointsPerCoin }, { c, v -> c.copy(pointsPerCoin = v) },
            { it.pointsPerCoinEnabled }, { c, v -> c.copy(pointsPerCoinEnabled = v) }),
        RateRow("Like (per like)", { it.pointsPerLike }, { c, v -> c.copy(pointsPerLike = v) },
            { it.pointsPerLikeEnabled }, { c, v -> c.copy(pointsPerLikeEnabled = v) }),
        RateRow("Follow", { it.pointsPerFollow }, { c, v -> c.copy(pointsPerFollow = v) },
            { it.pointsPerFollowEnabled }, { c, v -> c.copy(pointsPerFollowEnabled = v) }),
        RateRow("Share", { it.pointsPerShare }, { c, v -> c.copy(pointsPerShare = v) },
            { it.pointsPerShareEnabled }, { c, v -> c.copy(pointsPerShareEnabled = v) }),
        RateRow("Join", { it.pointsPerJoin }, { c, v -> c.copy(pointsPerJoin = v) },
            { it.pointsPerJoinEnabled }, { c, v -> c.copy(pointsPerJoinEnabled = v) }),
    )

    private data class BoundRow(val rate: TextInputEditText, val enabled: CheckBox)
    private val bound = mutableListOf<BoundRow>()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_points, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val boardCard: CollapsibleCard = view.findViewById(R.id.boardCard)
        val boardEmpty: EmptyStateView = view.findViewById(R.id.boardEmpty)
        val boardList: RecyclerView = view.findViewById(R.id.boardList)
        val ratesBox: LinearLayout = view.findViewById(R.id.ratesBox)
        val currencyInput: TextInputEditText = view.findViewById(R.id.currencyInput)
        val levelInput: TextInputEditText = view.findViewById(R.id.levelInput)
        val saveRatesButton: Button = view.findViewById(R.id.saveRatesButton)
        val adjustUser: TextInputEditText = view.findViewById(R.id.adjustUser)
        val adjustAmount: TextInputEditText = view.findViewById(R.id.adjustAmount)
        val adjustButton: Button = view.findViewById(R.id.adjustButton)
        val resetButton: Button = view.findViewById(R.id.resetButton)

        boardList.layoutManager = LinearLayoutManager(requireContext())
        boardList.addMinimalDividers()
        val adapter = PointsAdapter()
        boardList.adapter = adapter

        // Build the six rate rows once; values follow the config flow below.
        val inflater = LayoutInflater.from(requireContext())
        bound.clear()
        for (row in rows) {
            val rowView = inflater.inflate(R.layout.item_rate, ratesBox, false)
            rowView.findViewById<TextView>(R.id.rateLabel).text = row.label
            bound.add(
                BoundRow(
                    rate = rowView.findViewById(R.id.rateInput),
                    enabled = rowView.findViewById(R.id.rateEnabled),
                ),
            )
            ratesBox.addView(rowView)
        }

        saveRatesButton.setOnClickListener {
            var config = vm.points.config.value
            rows.forEachIndexed { i, row ->
                val rate = bound[i].rate.text.toString().toDoubleOrNull()
                if (rate != null) config = row.setRate(config, rate)
                config = row.setEnabled(config, bound[i].enabled.isChecked)
            }
            val currency = currencyInput.text.toString()
            if (currency.isNotBlank()) config = config.copy(currencyName = currency.trim())
            val level = levelInput.text.toString().toDoubleOrNull()
            if (level != null) config = config.copy(pointsPerLevel = level)
            vm.points.updateConfig(config)
            toast("rates saved")
        }
        adjustButton.setOnClickListener {
            val user = adjustUser.text.toString()
            val amount = adjustAmount.text.toString().toDoubleOrNull()
            if (user.isBlank() || amount == null) {
                toast("type a handle and an amount")
                return@setOnClickListener
            }
            viewLifecycleOwner.lifecycleScope.launch {
                val award = vm.points.adjust(user, amount)
                toast(if (award == null) "nothing adjusted" else "now ${PointsAdapter.formatPoints(award.total)}")
                adjustUser.setText("")
                adjustAmount.setText("")
            }
        }
        resetButton.setOnClickListener {
            vm.points.reset()
            toast("all balances reset")
        }

        val boardLabel = getString(R.string.board_label)
        val scope = viewLifecycleOwner.lifecycleScope
        scope.launch {
            viewLifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch {
                    vm.points.leaderboard.collect { viewers ->
                        adapter.submitList(viewers)
                        boardEmpty.visibility = if (viewers.isEmpty()) View.VISIBLE else View.GONE
                        boardCard.setTitleWithCount(boardLabel, viewers.size)
                    }
                }
                launch {
                    vm.points.config.collect { config ->
                        adapter.setCurrency(config.currencyName)
                        // Don't fight the user while they type: only fill
                        // untouched fields.
                        rows.forEachIndexed { i, row ->
                            val rateField = bound[i].rate
                            if (!rateField.hasFocus() && rateField.text.toString() != row.getRate(config).toString()) {
                                rateField.setText(row.getRate(config).toString())
                            }
                            val enabledBox = bound[i].enabled
                            if (enabledBox.isChecked != row.getEnabled(config)) {
                                enabledBox.isChecked = row.getEnabled(config)
                            }
                        }
                        if (!currencyInput.hasFocus() && currencyInput.text.toString() != config.currencyName) {
                            currencyInput.setText(config.currencyName)
                        }
                        if (!levelInput.hasFocus()) {
                            levelInput.setText(config.pointsPerLevel.toString())
                        }
                    }
                }
            }
        }
    }

    private fun toast(msg: String) {
        Toast.makeText(requireContext(), msg, Toast.LENGTH_SHORT).show()
    }
}
