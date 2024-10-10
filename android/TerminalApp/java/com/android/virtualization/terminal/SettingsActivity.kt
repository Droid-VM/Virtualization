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
import android.util.DisplayMetrics
import androidx.appcompat.app.AppCompatActivity
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView

class SettingsActivity : AppCompatActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.settings_activity)

        val settingsItems = arrayOf(
            SettingsItem(
                "Disk Resize",
                "Resize / Rootfs",
                R.drawable.baseline_storage_24,
                SettingsItemEnum.DiskResize
            ),
            SettingsItem(
                "Port Forwarding",
                "Configure port forwarding",
                R.drawable.baseline_call_missed_outgoing_24,
                SettingsItemEnum.PortForwarding
            ),
            SettingsItem(
                "Recovery",
                "Partition Recovery options",
                R.drawable.baseline_settings_backup_restore_24,
                SettingsItemEnum.Recovery
            ),
        )
        val displayMetrics: DisplayMetrics = this.getResources().displayMetrics
        val dpWidth = displayMetrics.widthPixels / displayMetrics.density
        val settingsListItemAdapter = SettingsItemAdapter(settingsItems, dpWidth)

        val recyclerView: RecyclerView = findViewById(R.id.settings_list_recycler_view)
        recyclerView.layoutManager = LinearLayoutManager(this)
        recyclerView.adapter = settingsListItemAdapter
    }
}