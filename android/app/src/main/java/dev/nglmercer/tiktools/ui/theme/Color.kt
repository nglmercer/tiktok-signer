package dev.nglmercer.tiktools.ui.theme

import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

data class TikToolsSemanticColors(
    val success: Color,
    val warning: Color,
    val error: Color,
    val live: Color,
)

val LocalSemanticColors = staticCompositionLocalOf {
    TikToolsSemanticColors(
        Color(0xFF176B48),
        Color(0xFF815600),
        Color(0xFFBA1A1A),
        Color(0xFF176B48),
    )
}
