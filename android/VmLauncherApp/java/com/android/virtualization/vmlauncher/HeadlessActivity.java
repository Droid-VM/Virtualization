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

package com.android.virtualization.vmlauncher;

import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.util.Log;
import android.webkit.*;
import android.widget.EditText;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStreamReader;
import java.util.Objects;

public class HeadlessActivity extends VmLauncherActivity {
    private static final String TAG = "HeadlessVmLauncherApp";
    private String mVmIpAddr;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_headless);
        WebView webView = (WebView) findViewById(R.id.webview);
        webView.getSettings().setAppCacheEnabled(true);
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
        new Thread(
                        () -> {
                            while (!getIpAddrFromVm()) {
                                try {
                                    Thread.sleep(1000);
                                } catch (Exception e) {
                                    Log.e(TAG, e.toString());
                                }
                            }
                        })
                .start();
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

    private boolean getIpAddrFromVm() {
        int INTERNAL_VSOCK_SERVER_PORT = 1024;
        try (ParcelFileDescriptor pfd = mVirtualMachine.connectVsock(INTERNAL_VSOCK_SERVER_PORT)) {
            try (BufferedReader input =
                    new BufferedReader(
                            new InputStreamReader(new FileInputStream(pfd.getFileDescriptor())))) {
                mVmIpAddr = input.readLine().strip();
                ((TextView) findViewById(R.id.ipaddrTextView)).setText(mVmIpAddr);
                findViewById(R.id.shellBtn).setEnabled(true);
                findViewById(R.id.goBtn).setEnabled(true);
                Thread.sleep(3000);
                gotoURL("http://", ":8888/?hostname=localhost&username=linux&password=bGludXg=");
                return true;
            } catch (IOException e) {
                Log.e(TAG, e.toString());
            }
        } catch (Exception e) {
            Log.e(TAG, e.toString());
        }
        return false;
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
}
