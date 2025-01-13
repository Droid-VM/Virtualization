package com.android.virtualization.terminal

import android.os.Bundle
import android.view.SurfaceView
import android.view.WindowInsets
import android.view.WindowInsetsController

class DisplayActivity : BaseActivity() {
    private lateinit var displayProvider: DisplayProvider

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_display)
        val mainView = findViewById<SurfaceView>(R.id.surface_view)
        val cursorView = findViewById<SurfaceView>(R.id.cursor_surface_view)
        makeFullscreen()
        // Connect the views to the VM
        displayProvider = DisplayProvider(mainView, cursorView)
    }

    override fun onPause() {
        super.onPause()
        displayProvider.notifyDisplayIsGoingToInvisible()
    }

    private fun makeFullscreen() {
        val w = window
        w.setDecorFitsSystemWindows(false)
        val insetsCtrl = w.insetsController
        insetsCtrl?.hide(WindowInsets.Type.systemBars())
        insetsCtrl?.setSystemBarsBehavior(
            WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        )
    }
}
