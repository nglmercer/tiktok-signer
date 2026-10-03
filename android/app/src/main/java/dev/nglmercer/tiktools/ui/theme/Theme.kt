package dev.nglmercer.tiktools.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import dev.nglmercer.tiktools.data.preferences.ThemeMode

@Composable
fun TikToolsTheme(mode: ThemeMode = ThemeMode.SYSTEM, content: @Composable () -> Unit) {
    val dark =
        when (mode) {
            ThemeMode.SYSTEM -> isSystemInDarkTheme()
            ThemeMode.LIGHT -> false
            ThemeMode.DARK -> true
        }
    val context = LocalContext.current
    val scheme =
        if (Build.VERSION.SDK_INT >= 31) {
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        } else if (dark) darkColorScheme(primary = Color(0xFFB0D9C4))
        else lightColorScheme(primary = Color(0xFF176B48))
    val semantic =
        TikToolsSemanticColors(
            if (dark) Color(0xFF79D9A7) else Color(0xFF176B48),
            if (dark) Color(0xFFFFCF80) else Color(0xFF815600),
            scheme.error,
            if (dark) Color(0xFF79D9A7) else Color(0xFF176B48),
        )
    CompositionLocalProvider(LocalSemanticColors provides semantic) {
        MaterialTheme(
            colorScheme = scheme,
            typography = TikToolsTypography,
            shapes = TikToolsShapes,
            content = content,
        )
    }
}
