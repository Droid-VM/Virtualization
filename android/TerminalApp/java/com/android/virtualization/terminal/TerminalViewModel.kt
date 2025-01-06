package com.android.virtualization.terminal

import androidx.lifecycle.ViewModel

class TerminalViewModel: ViewModel() {
    val terminalViews: MutableSet<TerminalView> = mutableSetOf()
}