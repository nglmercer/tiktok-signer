package com.example.ttlsigner.tts.supertonic

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** SuperTonic 3 manifest shape and download progress math. Pure JVM tests. */
class SupertonicModelsTest {

    @Test
    fun manifestHasSixRuntimeFilesPlusOneStyle() {
        assertEquals(7, SupertonicModels.FILES.size)
        val locals = SupertonicModels.FILES.map { it.local }
        assertEquals(locals.size, locals.toSet().size)
        for (name in listOf(
            "duration_predictor.onnx",
            "text_encoder.onnx",
            "vector_estimator.onnx",
            "vocoder.onnx",
            "tts.json",
            "unicode_indexer.json",
            SupertonicModels.STYLE_FILE,
        )) {
            assertTrue("missing $name", name in locals)
        }
    }

    @Test
    fun urlsPointAtSupertonic3Repo() {
        for (spec in SupertonicModels.FILES) {
            val url = SupertonicModels.url(spec)
            assertTrue(url, url.startsWith(SupertonicModels.BASE_URL))
            assertTrue(url, url.endsWith(spec.remote.substringAfterLast('/')))
        }
        assertTrue(SupertonicModels.BASE_URL.contains("Supertone/supertonic-3"))
    }

    @Test
    fun progressSpreadsEvenlyAcrossFiles() {
        val count = SupertonicModels.FILES.size
        // First file half done.
        assertEquals(
            0.5f / count,
            SupertonicModels.Progress(0, count, "a", 50, 100).overall(),
            0.0001f,
        )
        // Third file done (index 2 fully + fraction 1 of current file 3? no:
        // index counts finished files, fraction the current one).
        assertEquals(
            3f / count,
            SupertonicModels.Progress(2, count, "c", 100, 100).overall(),
            0.0001f,
        )
    }

    @Test
    fun progressToleratesUnknownTotals() {
        val count = SupertonicModels.FILES.size
        assertEquals(
            1f / count,
            SupertonicModels.Progress(1, count, "b", 500, -1).overall(),
            0.0001f,
        )
    }

    @Test
    fun progressClamps() {
        val count = SupertonicModels.FILES.size
        assertEquals(1f, SupertonicModels.Progress(count - 1, count, "z", 200, 100).overall(), 0f)
        assertEquals(0f, SupertonicModels.Progress(0, count, "a", -5, 100).overall(), 0f)
    }
}
