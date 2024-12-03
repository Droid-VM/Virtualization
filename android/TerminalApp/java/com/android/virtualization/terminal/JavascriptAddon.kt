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

import android.content.res.AssetManager
import android.webkit.WebView

object JavascriptAddon {
    private var ctrlKeyHandler: String? = null
    private var enableCtrlKey: String? = null
    private var touchToMouseHandler: String? = null

    private fun readCodeFromAsset(assetManager: AssetManager, fileName: String): String {
        return assetManager.open(fileName).bufferedReader().use { it.readText() }
    }

    fun mapTouchToMouseEvent(webView: WebView) {
        touchToMouseHandler =
            touchToMouseHandler
                ?: readCodeFromAsset(webView.context.assets, "js/touch_to_mouse_handler.js")
        webView.evaluateJavascript(touchToMouseHandler!!, null)
    }

    fun mapCtrlKey(webView: WebView) {
        ctrlKeyHandler =
            ctrlKeyHandler ?: readCodeFromAsset(webView.context.assets, "js/ctrl_key_handler.js")
        webView.evaluateJavascript(ctrlKeyHandler!!, null)
    }

    fun enableCtrlKey(webView: WebView) {
        enableCtrlKey =
            enableCtrlKey ?: readCodeFromAsset(webView.context.assets, "js/enable_ctrl_key.js")
        webView.evaluateJavascript(enableCtrlKey!!, null)
    }
}
