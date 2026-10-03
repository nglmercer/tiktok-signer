package dev.nglmercer.tiktools.tts.supertonic

import java.io.File
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/**
 * Unicode text processing for SuperTonic 3. Runs on the JVM against a temp indexer file — no ONNX
 * session, no Android.
 */
class UnicodeProcessorTest {

    @get:Rule val folder = TemporaryFolder()

    /** Identity indexer: code point N maps to id N. */
    private fun processor(size: Int = 1024): UnicodeProcessor {
        val file = folder.newFile("unicode_indexer.json")
        file.writeText((0 until size).joinToString(",", "[", "]"))
        return UnicodeProcessor(file)
    }

    @Test
    fun preprocessNormalizesPunctuationAndSpacing() {
        val p = processor()
        assertEquals("Hello world.", p.preprocess("  Hello_world  "))
        assertEquals("\"Hi\" 'there'.", p.preprocess("\u201cHi\u201d \u2018there\u2019"))
        assertEquals("a-b-c.", p.preprocess("a\u2013b\u2014c"))
    }

    @Test
    fun preprocessStripsCombiningMarks() {
        val p = processor()
        // e + combining acute normalizes to plain e after mark stripping.
        assertEquals("Cafe.", p.preprocess("Cafe\u0301"))
    }

    @Test
    fun preprocessKeepsTerminalPunctuation() {
        val p = processor()
        assertEquals("Really?", p.preprocess("Really?"))
        assertEquals("Wow!", p.preprocess("Wow!"))
        assertEquals("Done.", p.preprocess("Done."))
    }

    @Test
    fun processMapsCodePointsThroughIndexer() {
        val p = processor()
        val result = p.process(listOf("Hi"))
        // "Hi" + appended period.
        assertArrayEquals(longArrayOf(72, 105, 46), result.textIds[0])
        assertEquals(1, result.textMask.size)
        assertEquals(1, result.textMask[0].size)
        assertArrayEquals(floatArrayOf(1f, 1f, 1f), result.textMask[0][0], 0f)
    }

    @Test
    fun processPadsBatchToLongestWithZeroMask() {
        val p = processor()
        val result = p.process(listOf("Hi", "Hello!"))
        assertEquals(2, result.textIds.size)
        // "Hi." padded to length 6; "Hello!" needs no period.
        assertArrayEquals(longArrayOf(72, 105, 46, 0, 0, 0), result.textIds[0])
        assertEquals(72, result.textIds[1][0])
        assertArrayEquals(floatArrayOf(1f, 1f, 1f, 0f, 0f, 0f), result.textMask[0][0], 0f)
        assertArrayEquals(floatArrayOf(1f, 1f, 1f, 1f, 1f, 1f), result.textMask[1][0], 0f)
    }

    @Test
    fun processClampsOutOfRangeCodePoints() {
        val p = processor(size = 128)
        val result = p.process(listOf("A\uD83D\uDE00")) // emoji beyond the indexer
        assertEquals(65, result.textIds[0][0])
        assertEquals(127, result.textIds[0][1]) // clamped to lastIndex
    }

    @Test
    fun indexerFileMustExist() {
        try {
            UnicodeProcessor(File(folder.root, "missing.json"))
            assertTrue("expected an exception for a missing indexer", false)
        } catch (e: Exception) {
            // Any failure mode is fine — construction must not silently succeed.
        }
    }
}
