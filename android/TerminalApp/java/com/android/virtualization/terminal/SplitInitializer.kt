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

import android.content.ComponentName
import android.content.Context
import androidx.startup.Initializer
import androidx.window.embedding.RuleController
import androidx.window.embedding.SplitPairRule
import com.android.system.virtualmachine.flags.Flags

class SplitInitializer : Initializer<RuleController> {

    override fun create(context: Context): RuleController {
        val ruleController = RuleController.getInstance(context)
        val rules = RuleController.parseRules(context, R.xml.main_split_config).toMutableSet()

        // Remove SettingsDiskResizeActivity if storage ballooning is enabled.
        if (Flags.terminalStorageBalloon()) {
            val iterator = rules.iterator()
            while (iterator.hasNext()) {
                val rule = iterator.next()
                if (rule is SplitPairRule) {
                    val filters = rule.filters
                    val filterToRemove =
                        filters.firstOrNull { filter ->
                            filter.secondaryActivityName ==
                                ComponentName(context, SETTINGS_DISK_RESIZE_ACTIVITY_CLASS_NAME)
                        }
                    if (filterToRemove != null) {
                        iterator.remove()
                    }
                }
            }
        }
        ruleController.setRules(rules)
        return ruleController
    }

    override fun dependencies(): List<Class<out Initializer<*>>> {
        return emptyList()
    }

    companion object {
        private const val SETTINGS_DISK_RESIZE_ACTIVITY_CLASS_NAME =
            "com.android.virtualization.terminal.SettingsDiskResizeActivity"
    }
}
