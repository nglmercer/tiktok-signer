package dev.nglmercer.tiktools.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nglmercer.tiktools.ui.theme.TikToolsTheme

class MainActivity : ComponentActivity() {
    private val notificationGrant =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}
    private val container
        get() = (application as TikToolsApplication).container

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val preferences = container.preferences.state.collectAsStateWithLifecycle().value
            TikToolsTheme(preferences.theme) { TikToolsApp(container) }
        }
        if (
            Build.VERSION.SDK_INT >= 33 &&
                ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
                    PackageManager.PERMISSION_GRANTED
        )
            notificationGrant.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    override fun onStop() {
        super.onStop()
        if (!isChangingConfigurations) container.live.onBackground()
    }
}
