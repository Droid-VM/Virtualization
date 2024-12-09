/*
 * Copyright (C) 2024 The Android Open Source Project
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

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView

class SettingsPortForwardingActivity : AppCompatActivity() {
    private lateinit var mPortsStateManager: PortsStateManager
    private lateinit var mActivePortsAdapter: SettingsPortForwardingActiveAdapter
    private lateinit var mInactivePortsAdapter: SettingsPortForwardingInactiveAdapter

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.settings_port_forwarding)

        mPortsStateManager = PortsStateManager.getInstance(this)

        mActivePortsAdapter = SettingsPortForwardingActiveAdapter(mPortsStateManager)
        val activeRecyclerView: RecyclerView =
            findViewById(R.id.settings_port_forwarding_active_recycler_view)
        activeRecyclerView.layoutManager = LinearLayoutManager(this)
        activeRecyclerView.adapter = mActivePortsAdapter

        mInactivePortsAdapter = SettingsPortForwardingInactiveAdapter(mPortsStateManager)
        val inactiveRecyclerView: RecyclerView =
            findViewById(R.id.settings_port_forwarding_inactive_recycler_view)
        inactiveRecyclerView.layoutManager = LinearLayoutManager(this)
        inactiveRecyclerView.adapter = mInactivePortsAdapter
    }

    override fun onResume() {
        super.onResume()
        mActivePortsAdapter.registerPortsStateListener()
        mInactivePortsAdapter.registerPortsStateListener()
    }

    override fun onPause() {
        mInactivePortsAdapter.unregisterPortsStateListener()
        mActivePortsAdapter.unregisterPortsStateListener()
        super.onPause()
    }
}
