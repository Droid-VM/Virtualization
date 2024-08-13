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
package com.android.virtualization.vmterminal;

import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.webkit.WebChromeClient;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.EditText;
import android.widget.TextView;

import com.android.virtualization.vmlauncher.VmLauncherServices;

import java.util.Objects;

public class MainActivity extends Activity implements VmLauncherServices.VmLauncherServiceCallback {
    private static final String TAG = "VmTerminalApp";
    private String mVmIpAddr;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        VmLauncherServices.startVmLauncherService(this, this);

        setContentView(R.layout.activity_headless);
        WebView webView = (WebView) findViewById(R.id.webview);
        webView.getSettings().setDatabaseEnabled(true);
        webView.getSettings().setDomStorageEnabled(true);
        webView.getSettings().setJavaScriptEnabled(true);
        webView.setWebChromeClient(new WebChromeClient());
        webView.setWebViewClient(
                new WebViewClient() {
                    @Override
                    public boolean shouldOverrideUrlLoading(WebView view, String url) {
                        view.loadUrl(url);
                        return true;
                    }
                });
        findViewById(R.id.shellBtn)
                .setOnClickListener(
                        (v) -> {
                            gotoURL(
                                    "http://",
                                    ":8888/?hostname=localhost&username=linux&password=bGludXg=");
                        });
        findViewById(R.id.goBtn)
                .setOnClickListener(
                        (v) -> {
                            String prefix =
                                    Objects.toString(
                                            ((EditText) findViewById(R.id.urlPrefixEditText))
                                                    .getText(),
                                            "");
                            String suffix =
                                    Objects.toString(
                                            ((EditText) findViewById(R.id.urlSuffixEditText))
                                                    .getText(),
                                            "");
                            gotoURL(prefix, suffix);
                        });
    }

    private void gotoURL(String prefix, String suffix) {
        if (prefix == null || prefix.isEmpty()) {
            prefix = "http://";
        }
        if (suffix == null || suffix.isEmpty()) {
            suffix = ":8080";
        }
        String url = prefix + mVmIpAddr + suffix;
        runOnUiThread(() -> ((WebView) findViewById(R.id.webview)).loadUrl(url));
    }

    public void onVmStart(String vmName) {
        Log.i(TAG, "onVmStart(" + vmName + ")");
    }

    public void onVmStop() {
        Log.i(TAG, "onVmStop()");
        setResult(RESULT_OK);
        finish();
    }

    public void onVmError() {
        Log.i(TAG, "onVmError()");
        setResult(RESULT_CANCELED);
        finish();
    }

    public void onIpAddrAvailable(String ipAddr) {
        mVmIpAddr = ipAddr;
        ((TextView) findViewById(R.id.ipaddrTextView)).setText(mVmIpAddr);
        findViewById(R.id.shellBtn).setEnabled(true);
        findViewById(R.id.goBtn).setEnabled(true);
        new Handler(Looper.getMainLooper())
                .postDelayed(
                        () ->
                                gotoURL(
                                        "http://",
                                        ":8888/?hostname=localhost&username=linux&password=bGludXg="),
                        2000);
    }
}
