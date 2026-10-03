package dev.nglmercer.tiktools.app

import android.app.Application

class TikToolsApplication : Application() {
    val container: AppContainer by lazy { AppContainer(this) }
}
