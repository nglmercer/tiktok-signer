package dev.nglmercer.tiktools.feature

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

abstract class StudioViewModel : ViewModel() {
    private val _error = MutableStateFlow<String?>(null)
    val error = _error.asStateFlow()

    fun dismissError() {
        _error.value = null
    }

    protected fun task(block: suspend () -> Unit) =
        viewModelScope.launch {
            _error.value = null
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _error.value = e.message ?: "Operation failed"
            }
        }
}
