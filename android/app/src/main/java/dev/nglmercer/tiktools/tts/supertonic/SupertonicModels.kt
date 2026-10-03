package dev.nglmercer.tiktools.tts.supertonic

import android.content.Context
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.*
import kotlinx.coroutines.withContext

/**
 * The SuperTonic v3 file manifest and on-demand downloader. Model bytes never ship in the APK — the
 * Setup tab fetches them once from Hugging Face into `files/supertonic3/`, following nabu's
 * `supertonic-3-onnx` source (`Supertone/supertonic-3`). Only the v3 model is supported; the voice
 * is fixed to F1, exactly like the ported example's default path.
 */
object SupertonicModels {

    const val BASE_URL = "https://huggingface.co/Supertone/supertonic-3/resolve/main/"
    const val DIR_NAME = "supertonic3"
    const val STYLE_FILE = "style_F1.json"

    data class FileSpec(val remote: String, val local: String)

    /** The six runtime files plus the F1 voice style. */
    val FILES: List<FileSpec> =
        listOf(
            FileSpec("onnx/duration_predictor.onnx", "duration_predictor.onnx"),
            FileSpec("onnx/text_encoder.onnx", "text_encoder.onnx"),
            FileSpec("onnx/vector_estimator.onnx", "vector_estimator.onnx"),
            FileSpec("onnx/vocoder.onnx", "vocoder.onnx"),
            FileSpec("onnx/tts.json", "tts.json"),
            FileSpec("onnx/unicode_indexer.json", "unicode_indexer.json"),
            FileSpec("voice_styles/F1.json", STYLE_FILE),
        )

    fun url(spec: FileSpec): String = BASE_URL + spec.remote

    fun dir(context: Context): File = File(context.filesDir, DIR_NAME)

    fun styleFile(context: Context): File = File(dir(context), STYLE_FILE)

    sealed interface Status {
        data object Ready : Status

        data class Missing(val files: List<String>) : Status
    }

    /** Ready when every manifest file exists and is non-empty. */
    fun status(context: Context): Status {
        val missing =
            FILES.map { it.local }
                .filter { name ->
                    val file = File(dir(context), name)
                    !file.isFile || file.length() == 0L
                }
        return if (missing.isEmpty()) Status.Ready else Status.Missing(missing)
    }

    data class Progress(
        val fileIndex: Int,
        val fileCount: Int,
        val fileName: String,
        val bytesDone: Long,
        val bytesTotal: Long,
    ) {
        /** 0..1 across the whole manifest, weighting files equally. */
        fun overall(): Float = ((fileIndex + fraction()) / fileCount).coerceIn(0f, 1f)

        private fun fraction(): Float =
            if (bytesTotal <= 0) 0f else (bytesDone.toFloat() / bytesTotal).coerceIn(0f, 1f)
    }

    /**
     * Download the missing manifest files, skipping ones already on disk. Each file streams to
     * `.part` and is renamed on success, so a killed download never leaves a half file behind.
     * [onProgress] runs on the caller's thread — the view model forwards it to the UI flow.
     */
    suspend fun download(context: Context, onProgress: (Progress) -> Unit = {}): Unit =
        withContext(Dispatchers.IO) {
            val target = dir(context)
            if (!target.isDirectory) target.mkdirs()
            FILES.forEachIndexed { index, spec ->
                val out = File(target, spec.local)
                if (out.isFile && out.length() > 0) {
                    onProgress(Progress(index, FILES.size, spec.local, 1, 1))
                    return@forEachIndexed
                }
                fetch(URL(url(spec)), out) { done, total ->
                    onProgress(Progress(index, FILES.size, spec.local, done, total))
                }
            }
        }

    private suspend fun fetch(url: URL, out: File, onBytes: (done: Long, total: Long) -> Unit) =
        coroutineScope {
            val tmp = File(out.parent, out.name + ".part")
            val connection = url.openConnection() as HttpURLConnection
            val cancellation =
                launch(Dispatchers.IO) {
                    try {
                        awaitCancellation()
                    } finally {
                        connection.disconnect()
                    }
                }
            try {
                ensureActive()
                connection.connectTimeout = 15_000
                connection.readTimeout = 60_000
                connection.setRequestProperty("User-Agent", "TikTools-Studio/1.0")
                connection.connect()
                val code = connection.responseCode
                check(code in 200..299) { "HTTP $code for ${url.path.substringAfterLast('/')}" }
                val total = connection.contentLengthLong.takeIf { it > 0 } ?: -1L
                var done = 0L
                var lastReport = 0L
                connection.inputStream.use { input ->
                    FileOutputStream(tmp).use { output ->
                        val buf = ByteArray(64 * 1024)
                        while (true) {
                            ensureActive()
                            val n = input.read(buf)
                            if (n <= 0) break
                            output.write(buf, 0, n)
                            done += n
                            if (done - lastReport >= 512 * 1024) {
                                lastReport = done
                                onBytes(done, total)
                            }
                        }
                    }
                }
                onBytes(done, total)
                check(tmp.renameTo(out)) { "could not save ${out.name}" }
            } finally {
                cancellation.cancel()
                connection.disconnect()
                if (tmp.isFile && !out.isFile) tmp.delete()
            }
        }
}
