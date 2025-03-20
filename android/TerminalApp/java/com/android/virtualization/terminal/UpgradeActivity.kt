package com.android.virtualization.terminal

import android.annotation.MainThread
import android.content.Intent
import android.os.Bundle
import android.util.Log
import android.view.View
import com.google.android.material.snackbar.Snackbar
import java.io.IOException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

class UpgradeActivity : BaseActivity() {
    private lateinit var executorService: ExecutorService

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        executorService =
            Executors.newSingleThreadExecutor(TerminalThreadFactory(applicationContext))

        setContentView(R.layout.activity_upgrade)

        val button = findViewById<View>(R.id.upgrade)
        button.setOnClickListener { _ -> upgrade() }
    }

    override fun onDestroy() {
        super.onDestroy()

        executorService.shutdown()
    }

    private fun upgrade() {
        findViewById<View>(R.id.progress).visibility = View.VISIBLE

        executorService.execute {
            val image = InstalledImage.getDefault(this)
            try {
                image.uninstallAndBackup()
            } catch (e: IOException) {
                Snackbar.make(
                        findViewById<View>(android.R.id.content),
                        R.string.upgrade_error,
                        Snackbar.LENGTH_SHORT,
                    )
                    .show()
                Log.e(MainActivity.Companion.TAG, "Failed to upgrade ", e)
                return@execute
            }

            runOnUiThread {
                findViewById<View>(R.id.progress).visibility = View.INVISIBLE
                restartTerminal()
            }
        }
    }

    @MainThread
    private fun restartTerminal() {
        val intent = baseContext.packageManager.getLaunchIntentForPackage(baseContext.packageName)
        intent?.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TASK)
        finish()
        startActivity(intent)
    }
}
