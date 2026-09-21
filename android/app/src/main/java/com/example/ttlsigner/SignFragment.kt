package com.example.ttlsigner

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.TextView
import androidx.fragment.app.Fragment
import androidx.fragment.app.activityViewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.google.android.material.progressindicator.LinearProgressIndicator
import com.google.android.material.textfield.TextInputEditText
import kotlinx.coroutines.launch

/** Sign tab: resolve + sign one handle, the result card, the debug console. */
class SignFragment : Fragment() {

    private val vm: SessionViewModel by activityViewModels()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = inflater.inflate(R.layout.fragment_sign, container, false)

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        val handleInput: TextInputEditText = view.findViewById(R.id.handleInput)
        val resolveButton: Button = view.findViewById(R.id.resolveButton)
        val signButton: Button = view.findViewById(R.id.signButton)
        val signProgress: LinearProgressIndicator = view.findViewById(R.id.signProgress)
        val resultPlaceholder: TextView = view.findViewById(R.id.resultPlaceholder)
        val resultRoom: TextView = view.findViewById(R.id.resultRoom)
        val resultUser: TextView = view.findViewById(R.id.resultUser)
        val resultSummary: TextView = view.findViewById(R.id.resultSummary)
        val resultLatency: TextView = view.findViewById(R.id.resultLatency)
        val resultError: TextView = view.findViewById(R.id.resultError)
        val resultFields = listOf(resultRoom, resultUser, resultSummary, resultLatency)

        resolveButton.setOnClickListener { vm.resolve(handleInput.text.toString()) }
        signButton.setOnClickListener { vm.sign(handleInput.text.toString()) }

        val scope = viewLifecycleOwner.lifecycleScope
        scope.launch {
            viewLifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch {
                    vm.busy.collect { busy ->
                        signProgress.visibility = if (busy) View.VISIBLE else View.GONE
                        resolveButton.isEnabled = !busy
                        signButton.isEnabled = !busy
                    }
                }
                launch {
                    // A feed tap suggests its handle — unless the user is typing.
                    vm.lastHandle.collect { handle ->
                        if (handle.isNotEmpty() && !handleInput.hasFocus() &&
                            handleInput.text.toString() != handle
                        ) {
                            handleInput.setText(handle)
                        }
                    }
                }
                launch {
                    vm.resolveInfo.collect { info ->
                        if (info == null) return@collect
                        resultPlaceholder.visibility = View.GONE
                        resultError.visibility = View.GONE
                        resultRoom.visibility = View.VISIBLE
                        resultUser.visibility = View.VISIBLE
                        resultRoom.text = "room ${info.roomId} · ${if (info.live) "LIVE" else "not live"}"
                        resultUser.text = "@${info.handle} (${info.nickname}) · ${info.title}"
                        resultLatency.visibility = View.VISIBLE
                        resultLatency.text = "resolve ${info.ms}ms"
                    }
                }
                launch {
                    vm.signResult.collect { result ->
                        if (result == null) return@collect
                        resultPlaceholder.visibility = View.GONE
                        resultError.visibility = View.GONE
                        resultRoom.visibility = View.VISIBLE
                        resultRoom.text = "room ${result.room}"
                        // A numeric sign has no user line; a resolve+sign keeps it.
                        val resolved = vm.resolveInfo.value
                        val showUser = resolved != null && resolved.roomId == result.room
                        resultUser.visibility = if (showUser) View.VISIBLE else View.GONE
                        resultSummary.visibility = View.VISIBLE
                        resultSummary.text = result.summary
                        resultLatency.visibility = View.VISIBLE
                        resultLatency.text = "sign ${result.ms}ms"
                    }
                }
                launch {
                    vm.taskError.collect { error ->
                        resultError.visibility = if (error == null) View.GONE else View.VISIBLE
                        resultError.text = error.orEmpty()
                        if (error != null) {
                            resultPlaceholder.visibility = View.GONE
                            resultFields.forEach { it.visibility = View.GONE }
                        }
                    }
                }
            }
        }
    }
}
