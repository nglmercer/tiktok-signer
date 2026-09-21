package com.example.ttlsigner.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.util.AttributeSet
import android.view.LayoutInflater
import android.view.View
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import com.example.ttlsigner.Logger
import com.example.ttlsigner.R

/**
 * The shared debug console: renders the [Logger] buffer, follows new lines,
 * copies or clears on demand. Subscribes while attached, so any tab can host
 * it with zero wiring — just drop it in a layout.
 */
class LogConsoleView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : LinearLayout(context, attrs, defStyleAttr) {

    private val output: TextView
    private val outputScroll: ScrollView
    private var hasContent = false

    private val listener = object : Logger.Listener {
        override fun onLine(line: String) {
            // Logger notifies on the caller's thread; hop to the UI thread.
            post {
                if (!hasContent) {
                    output.text = ""
                    hasContent = true
                }
                output.append(line + "\n")
                outputScroll.post { outputScroll.fullScroll(View.FOCUS_DOWN) }
            }
        }

        override fun onCleared() {
            post {
                hasContent = false
                output.text = context.getString(R.string.output_placeholder)
                outputScroll.fullScroll(View.FOCUS_UP)
            }
        }
    }

    init {
        orientation = VERTICAL
        LayoutInflater.from(context).inflate(R.layout.view_log_console, this, true)
        output = findViewById(R.id.output)
        outputScroll = findViewById(R.id.outputScroll)
        findViewById<View>(R.id.copyButton).setOnClickListener { copyLog() }
        findViewById<View>(R.id.clearButton).setOnClickListener { Logger.clear() }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        val buffered = Logger.snapshot()
        if (buffered.isEmpty()) {
            hasContent = false
            output.text = context.getString(R.string.output_placeholder)
        } else {
            hasContent = true
            output.text = buffered
            outputScroll.post { outputScroll.fullScroll(View.FOCUS_DOWN) }
        }
        Logger.addListener(listener)
    }

    override fun onDetachedFromWindow() {
        Logger.removeListener(listener)
        super.onDetachedFromWindow()
    }

    private fun copyLog() {
        val text = if (hasContent) output.text.toString() else ""
        val clip = ClipData.newPlainText("ttl-signer log", text)
        (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager)
            .setPrimaryClip(clip)
        Toast.makeText(context, "log copied (${text.length} chars)", Toast.LENGTH_SHORT).show()
    }
}
