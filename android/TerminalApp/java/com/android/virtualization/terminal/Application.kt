/*
 * Copyright 2025 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
package com.android.virtualization.terminal

import android.app.Application as AndroidApplication
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.android.virtualization.terminal.VmLauncherService.Companion.APP_ON_START
import com.android.virtualization.terminal.VmLauncherService.Companion.APP_ON_STOP

public class Application : AndroidApplication() {
    override fun onCreate() {
        super.onCreate()
        setupNotificationChannels()
        val lifecycleObserver = ApplicationLifecycleObserver(this)
        ProcessLifecycleOwner.get().lifecycle.addObserver(lifecycleObserver)
    }

    private fun setupNotificationChannels() {
        val nm = getSystemService<NotificationManager>(NotificationManager::class.java)

        nm.createNotificationChannel(
            NotificationChannel(
                CHANNEL_LONG_RUNNING_ID,
                getString(R.string.notification_channel_long_running_name),
                NotificationManager.IMPORTANCE_DEFAULT,
            )
        )

        nm.createNotificationChannel(
            NotificationChannel(
                CHANNEL_SYSTEM_EVENTS_ID,
                getString(R.string.notification_channel_system_events_name),
                NotificationManager.IMPORTANCE_HIGH,
            )
        )
    }

    companion object {
        const val CHANNEL_LONG_RUNNING_ID = "long_running"
        const val CHANNEL_SYSTEM_EVENTS_ID = "system_events"
        const val APP_ENTER_BACKGROUND_EVENT = "android.virtualization.APP_ENTER_BACKGROUND_EVENT"
        const val APP_ENTER_FOREGROUND_EVENT = "android.virtualization.APP_ENTER_FOREGROUND_EVENT"

        fun getInstance(c: Context): Application = c.getApplicationContext() as Application
    }

    class ApplicationLifecycleObserver(private val app: Application) : DefaultLifecycleObserver {
        private var vmLauncherService: VmLauncherService? = null
        private var isBound = false
        private val connection =
            object : ServiceConnection {
                override fun onServiceConnected(className: ComponentName, service: IBinder) {
                    val binder = service as VmLauncherService.VmLauncherServiceBinder
                    vmLauncherService = binder.getService()
                    isBound = true
                    // Service is bound, you can now interact with it
                    println("yuan: Service Bound")
                }

                override fun onServiceDisconnected(arg0: ComponentName) {
                    isBound = false
                    vmLauncherService = null
                    println("yuan: Service Disconnected")
                }
            }

        override fun onCreate(owner: LifecycleOwner) {
            super.onCreate(owner)
            bindToVmLauncherService()
        }

        override fun onStart(owner: LifecycleOwner) {
            super.onStart(owner)
            println("yuan: Application entered foreground")
            if (isBound) {
                println("yuan: send service hello")
                vmLauncherService?.processCommand(APP_ON_START)
            }
        }

        override fun onStop(owner: LifecycleOwner) {
            println("yuan: Application entered background")
            if (isBound) {
                println("yuan: send service goodbye")
                vmLauncherService?.processCommand(APP_ON_STOP)
            }
            super.onStop(owner)
        }

        override fun onDestroy(owner: LifecycleOwner) {
            if (isBound) {
                app.unbindService(connection)
                isBound = false
                vmLauncherService = null
                println("yuan: Application unbounded")
            }
            super.onDestroy(owner)
        }

        fun bindToVmLauncherService() {
            val intent = Intent(app, VmLauncherService::class.java) // Simple Intent
            val bound = app.bindService(intent, connection, 0) // No BIND_AUTO_CREATE
        }
    }
}
