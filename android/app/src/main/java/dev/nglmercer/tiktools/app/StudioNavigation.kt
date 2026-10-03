package dev.nglmercer.tiktools.app

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.*
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.*

data class StudioDestination(val route: String, val label: String, val icon: ImageVector)

private val destinations =
    listOf(
        StudioDestination("home", "Home", Icons.Outlined.Home),
        StudioDestination("events", "Events", Icons.Outlined.ChatBubbleOutline),
        StudioDestination("actions", "Actions", Icons.Outlined.Bolt),
        StudioDestination("rewards", "Rewards", Icons.Outlined.EmojiEvents),
    )

/** Stateless screen slots make navigation testable without loading JNI or accessing TikTok. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StudioNavigation(
    home: @Composable () -> Unit,
    events: @Composable () -> Unit,
    actions: @Composable () -> Unit,
    rewards: @Composable () -> Unit,
    settings: @Composable ((String) -> Unit) -> Unit,
    speech: @Composable () -> Unit,
    diagnostics: @Composable () -> Unit,
) {
    val nav = rememberNavController()
    val current = nav.currentBackStackEntryAsState().value?.destination?.route ?: "home"
    val topLevel = current in destinations.map { it.route }
    fun open(route: String) {
        nav.navigate(route) {
            launchSingleTop = true
            if (route in destinations.map { it.route }) {
                restoreState = true
                popUpTo(nav.graph.findStartDestination().id) { saveState = true }
            }
        }
    }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= 600.dp
        Row(Modifier.fillMaxSize()) {
            if (wide && topLevel)
                NavigationRail(
                    Modifier.windowInsetsPadding(
                        WindowInsets.systemBars.only(WindowInsetsSides.Vertical)
                    )
                ) {
                    Spacer(Modifier.height(24.dp))
                    for (d in destinations) NavigationRailItem(
                        current == d.route,
                        onClick = { open(d.route) },
                        icon = { Icon(d.icon, null) },
                        label = { Text(d.label) },
                    )
                }
            Scaffold(
                Modifier.weight(1f),
                topBar = {
                    TopAppBar(
                        title = { Text("TikTools") },
                        navigationIcon = {
                            if (!topLevel)
                                IconButton(onClick = { nav.popBackStack() }) {
                                    Icon(Icons.Outlined.ArrowBack, "Back")
                                }
                        },
                        actions = {
                            if (topLevel)
                                IconButton(onClick = { open("settings") }) {
                                    Icon(Icons.Outlined.Settings, "Settings")
                                }
                        },
                    )
                },
                bottomBar = {
                    if (!wide && topLevel)
                        NavigationBar {
                            for (d in destinations) NavigationBarItem(
                                current == d.route,
                                onClick = { open(d.route) },
                                icon = { Icon(d.icon, null) },
                                label = { Text(d.label) },
                            )
                        }
                },
            ) { padding ->
                Box(
                    Modifier.padding(padding).fillMaxSize(),
                    contentAlignment = Alignment.TopCenter,
                ) {
                    NavHost(nav, "home", Modifier.widthIn(max = 900.dp).fillMaxSize()) {
                        composable("home") { home() }
                        composable("events") { events() }
                        composable("actions") { actions() }
                        composable("rewards") { rewards() }
                        composable("settings") { settings(::open) }
                        composable("speech") { speech() }
                        composable("diagnostics") { diagnostics() }
                    }
                }
            }
        }
    }
}
