package com.example.ttlsigner

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.Spinner
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.fragment.app.Fragment
import androidx.fragment.app.activityViewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.example.ttlsigner.actions.EventAction
import com.example.ttlsigner.events.LiveEvent
import com.example.ttlsigner.ui.CollapsibleCard
import com.google.android.material.chip.Chip
import com.google.android.material.chip.ChipGroup
import com.google.android.material.textfield.TextInputEditText
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.launch

/**
 * Actions tab: the fetch-only automations. Each action fires one HTTP fetch
 * when a selected event category lands; the run log shows what fired, where,
 * and how the server answered. The "+" button opens the editor sheet.
 */
class ActionsFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_actions, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val actionsCard: CollapsibleCard = view.findViewById(R.id.actionsCard)
        val actionsEmpty: TextView = view.findViewById(R.id.actionsEmpty)
        val actionsList: RecyclerView = view.findViewById(R.id.actionsList)
        val addButton: Button = view.findViewById(R.id.addActionButton)
        val runsText: TextView = view.findViewById(R.id.runsText)

        actionsList.layoutManager = LinearLayoutManager(requireContext())
        val adapter = ActionsAdapter(object : ActionsAdapter.Listener {
            override fun onToggle(action: EventAction, enabled: Boolean) {
                vm.actions.setEnabled(action, enabled)
            }
            override fun onEdit(action: EventAction) = openEditor(action)
            override fun onTest(action: EventAction) {
                // A synthetic chat event: the URL/body render with sample data
                // so the run log shows exactly what would be fetched.
                vm.actions.test(action, LiveEvent(LiveEvent.Category.CHAT, "sample_user", "Sample", "hello", 0, 0))
                toast("test fired — see the run log")
            }
            override fun onDelete(action: EventAction) {
                vm.actions.delete(action.id)
                toast("action deleted")
            }
        })
        actionsList.adapter = adapter
        addButton.setOnClickListener { openEditor(null) }

        val actionsLabel = getString(R.string.actions_label)
        val scope = viewLifecycleOwner.lifecycleScope
        scope.launch {
            viewLifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch {
                    vm.actions.state.collect { state ->
                        adapter.submitList(state.actions)
                        actionsEmpty.visibility =
                            if (state.actions.isEmpty()) View.VISIBLE else View.GONE
                        actionsCard.setTitle("$actionsLabel (${state.actions.size})")
                        runsText.text = if (state.runs.isEmpty()) {
                            getString(R.string.runs_empty)
                        } else {
                            state.runs.joinToString("\n") { run ->
                                "${CLOCK.format(Date(run.at))} [${run.status}] ${run.detail}"
                            }
                        }
                    }
                }
            }
        }
    }

    /** The action editor: name, triggers, method, URL/body templates, cooldown. */
    private fun openEditor(existing: EventAction?) {
        val dialogView = layoutInflater.inflate(R.layout.dialog_action_edit, null)
        val nameInput: TextInputEditText = dialogView.findViewById(R.id.actionNameInput)
        val triggerChips: ChipGroup = dialogView.findViewById(R.id.triggerChips)
        val methodSpinner: Spinner = dialogView.findViewById(R.id.methodSpinner)
        val urlInput: TextInputEditText = dialogView.findViewById(R.id.urlInput)
        val bodyInput: TextInputEditText = dialogView.findViewById(R.id.bodyInput)
        val cooldownInput: TextInputEditText = dialogView.findViewById(R.id.cooldownInput)

        // Trigger chips: the user-facing event kinds. ROOM/UNKNOWN fire too
        // often or too rarely to automate, so the editor omits them.
        val triggerKinds = listOf(
            LiveEvent.Category.CHAT,
            LiveEvent.Category.GIFT,
            LiveEvent.Category.LIKE,
            LiveEvent.Category.FOLLOW,
            LiveEvent.Category.SHARE,
            LiveEvent.Category.JOIN,
            LiveEvent.Category.MEMBER,
        )
        val current = existing ?: EventAction()
        for (kind in triggerKinds) {
            val chip = Chip(requireContext()).apply {
                text = kind.name.lowercase()
                isCheckable = true
                isChecked = kind in current.triggers
            }
            triggerChips.addView(chip)
        }
        methodSpinner.adapter = ArrayAdapter(
            requireContext(),
            android.R.layout.simple_spinner_dropdown_item,
            EventAction.Method.values().map { it.name },
        )
        methodSpinner.setSelection(current.method.ordinal)

        nameInput.setText(current.name)
        urlInput.setText(current.url)
        bodyInput.setText(current.body)
        if (current.cooldownSecs > 0) cooldownInput.setText(current.cooldownSecs.toString())

        AlertDialog.Builder(requireContext())
            .setTitle(if (existing == null) R.string.action_new else R.string.action_edit)
            .setView(dialogView)
            .setPositiveButton(android.R.string.ok) { _, _ ->
                val triggers = mutableSetOf<LiveEvent.Category>()
                for (i in 0 until triggerChips.childCount) {
                    val chip = triggerChips.getChildAt(i) as Chip
                    if (chip.isChecked) triggers.add(triggerKinds[i])
                }
                if (triggers.isEmpty()) {
                    toast("pick at least one trigger")
                    return@setPositiveButton
                }
                val url = urlInput.text.toString().trim()
                if (!url.startsWith("http://") && !url.startsWith("https://")) {
                    toast("the URL must start with http(s)://")
                    return@setPositiveButton
                }
                vm.actions.save(
                    current.copy(
                        name = nameInput.text.toString().trim().ifEmpty { "Untitled action" },
                        triggers = triggers,
                        method = EventAction.Method.values()[methodSpinner.selectedItemPosition],
                        url = url,
                        body = bodyInput.text.toString(),
                        cooldownSecs = cooldownInput.text.toString().toLongOrNull()?.coerceAtLeast(0) ?: 0,
                    ),
                )
                toast("action saved")
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun toast(msg: String) {
        Toast.makeText(requireContext(), msg, Toast.LENGTH_SHORT).show()
    }

    companion object {
        private val CLOCK = SimpleDateFormat("HH:mm:ss", Locale.US)
    }
}
