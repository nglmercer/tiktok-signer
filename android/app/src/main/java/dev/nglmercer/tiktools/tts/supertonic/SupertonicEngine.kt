package dev.nglmercer.tiktools.tts.supertonic

import ai.onnxruntime.OnnxTensor
import ai.onnxruntime.OrtEnvironment
import ai.onnxruntime.OrtSession
import java.io.File
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sqrt
import kotlin.random.Random
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * SuperTonic v3 on-device inference, ported from the nabu example
 * ([mewmix/nabu](https://github.com/mewmix/nabu), `app/.../supertonic/SupertonicEngine.kt`, itself
 * a Kotlin port of the Supertonic Java demo). Four ONNX Runtime CPU sessions over a [modelDir]
 * holding `duration_predictor.onnx`, `text_encoder.onnx`, `vector_estimator.onnx`, `vocoder.onnx`,
 * `tts.json`, and `unicode_indexer.json` from `Supertone/supertonic-3`.
 *
 * Only the v3 model is supported. Sessions are heavy (~1 GB RAM while loaded); callers keep one
 * engine and share it.
 */
class SupertonicEngine(
    private val modelDir: File,
    env: OrtEnvironment = OrtEnvironment.getEnvironment(),
) : AutoCloseable {
    private val config: SupertonicConfig = loadConfig(modelDir)
    val sampleRate: Int
        get() = config.sampleRate

    val name: String = "Supertonic-3"
    val provider: String = "CPU"
    private val textProcessor = UnicodeProcessor(File(modelDir, "unicode_indexer.json"))
    private val dpSession: OrtSession
    private val textEncSession: OrtSession
    private val vectorEstSession: OrtSession
    private val vocoderSession: OrtSession

    init {
        val opts = OrtSession.SessionOptions()
        dpSession = env.createSession(File(modelDir, "duration_predictor.onnx").absolutePath, opts)
        textEncSession = env.createSession(File(modelDir, "text_encoder.onnx").absolutePath, opts)
        vectorEstSession =
            env.createSession(File(modelDir, "vector_estimator.onnx").absolutePath, opts)
        vocoderSession = env.createSession(File(modelDir, "vocoder.onnx").absolutePath, opts)
    }

    suspend fun synthesize(
        text: String,
        style: SupertonicStyle,
        totalStep: Int = 5,
        speed: Float = 1.05f,
        rng: Random = Random.Default,
        env: OrtEnvironment = OrtEnvironment.getEnvironment(),
    ): SupertonicResult = synthesize(listOf(text), style, totalStep, speed, rng, env)

    suspend fun synthesize(
        texts: List<String>,
        style: SupertonicStyle,
        totalStep: Int = 5,
        speed: Float = 1.05f,
        rng: Random = Random.Default,
        env: OrtEnvironment = OrtEnvironment.getEnvironment(),
    ): SupertonicResult =
        withContext(Dispatchers.Default) {
            require(texts.isNotEmpty()) { "No input text" }
            val bsz = texts.size

            val textResult = textProcessor.process(texts)
            val textIdsTensor = createLongTensor(textResult.textIds, env)
            val textMaskTensor = createFloatTensor(textResult.textMask, env)

            val dpInputs =
                mapOf(
                    "text_ids" to textIdsTensor,
                    "style_dp" to style.dpTensor,
                    "text_mask" to textMaskTensor,
                )
            val duration =
                dpSession.run(dpInputs).use { res ->
                    val value = res[0].value
                    val raw =
                        when (value) {
                            is Array<*> -> (value as Array<FloatArray>)[0]
                            is FloatArray -> value
                            else ->
                                throw IllegalStateException(
                                    "Unexpected duration type ${value?.javaClass}"
                                )
                        }
                    raw.copyOf().also { arr -> for (i in arr.indices) arr[i] = arr[i] / speed }
                }

            val textEncInputs =
                mapOf(
                    "text_ids" to textIdsTensor,
                    "style_ttl" to style.ttlTensor,
                    "text_mask" to textMaskTensor,
                )
            val textEncResult = textEncSession.run(textEncInputs)
            val textEmbTensor = textEncResult[0] as OnnxTensor

            val latent = sampleNoisyLatent(duration, rng)
            val totalStepTensor =
                OnnxTensor.createTensor(env, FloatArray(bsz) { totalStep.toFloat() })

            var xt = latent.noisy
            val latentMask = latent.mask
            repeat(totalStep) { step ->
                val currentStepTensor =
                    OnnxTensor.createTensor(env, FloatArray(bsz) { step.toFloat() })
                val noisyLatentTensor = createFloatTensor(xt, env)
                val latentMaskTensor = createFloatTensor(latentMask, env)
                val textMaskTensor2 = createFloatTensor(textResult.textMask, env)

                val vectorInputs =
                    mapOf(
                        "noisy_latent" to noisyLatentTensor,
                        "text_emb" to textEmbTensor,
                        "style_ttl" to style.ttlTensor,
                        "latent_mask" to latentMaskTensor,
                        "text_mask" to textMaskTensor2,
                        "current_step" to currentStepTensor,
                        "total_step" to totalStepTensor,
                    )
                xt =
                    vectorEstSession.run(vectorInputs).use { res ->
                        @Suppress("UNCHECKED_CAST")
                        val out = res[0].value as Array<Array<FloatArray>>
                        out
                    }

                currentStepTensor.close()
                noisyLatentTensor.close()
                latentMaskTensor.close()
                textMaskTensor2.close()
            }

            val finalLatentTensor = createFloatTensor(xt, env)
            val wavBatch =
                vocoderSession.run(mapOf("latent" to finalLatentTensor)).use { res ->
                    res[0].value as Array<FloatArray>
                }

            textIdsTensor.close()
            textMaskTensor.close()
            textEncResult.close()
            totalStepTensor.close()
            finalLatentTensor.close()

            val wav = wavBatch.getOrNull(0) ?: FloatArray(0)
            SupertonicResult(wav = wav, duration = duration, sampleRate = config.sampleRate)
        }

    override fun close() {
        dpSession.close()
        textEncSession.close()
        vectorEstSession.close()
        vocoderSession.close()
    }

    private fun sampleNoisyLatent(duration: FloatArray, rng: Random): LatentBundle {
        val bsz = duration.size
        var maxDur = 0f
        for (d in duration) maxDur = max(maxDur, d)

        val wavLenMax = (maxDur * config.sampleRate).toLong()
        val wavLengths = LongArray(bsz) { (duration[it] * config.sampleRate).toLong() }

        val chunkSize = config.baseChunkSize * config.chunkCompress
        val latentLen = ((wavLenMax + chunkSize - 1) / chunkSize).toInt()
        val latentDim = config.latentDim * config.chunkCompress

        val noisy = Array(bsz) { Array(latentDim) { FloatArray(latentLen) } }
        for (b in 0 until bsz) {
            for (d in 0 until latentDim) {
                for (t in 0 until latentLen) {
                    val u1 = max(1e-10f, rng.nextFloat())
                    val u2 = rng.nextFloat()
                    noisy[b][d][t] =
                        (sqrt(-2.0f * kotlin.math.ln(u1)) *
                            kotlin.math.cos(2.0f * Math.PI.toFloat() * u2))
                }
            }
        }

        val mask = latentMask(wavLengths)
        for (b in 0 until bsz) {
            for (d in 0 until latentDim) {
                for (t in noisy[b][d].indices) {
                    noisy[b][d][t] *= mask[b][0][t]
                }
            }
        }

        return LatentBundle(noisy, mask)
    }

    private fun latentMask(wavLengths: LongArray): Array<Array<FloatArray>> {
        val latentSize = config.baseChunkSize.toLong() * config.chunkCompress.toLong()
        val latentLengths =
            LongArray(wavLengths.size) { (wavLengths[it] + latentSize - 1) / latentSize }
        val maxLen = latentLengths.maxOrNull() ?: 0

        return Array(wavLengths.size) { b ->
            Array(1) {
                FloatArray(maxLen.toInt()) { idx ->
                    val active = if (idx < latentLengths[b]) 1f else 0f
                    if (idx == 0) active else min(1f, active)
                }
            }
        }
    }
}
