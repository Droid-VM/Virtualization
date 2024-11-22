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
package com.android.virtualization.terminal;

import android.content.Context;
import android.util.AttributeSet;
import android.webkit.WebView;

public class TerminalView extends WebView {
    // keyCode 229 means composing text, so get the last character in e.target.value
    // 64(@)-95(_) have mapped ctrl codes, and 97(A)-122(Z) is mapped ctrl+small letter
    public static final String CTRL_KEY_HANDLER =
            """
            javascript: (function() {
              window.term.attachCustomKeyEventHandler((e) => {
                  console.log(e.type, e.keyCode, window.ctrl);
                  if (window.ctrl) {
                      console.log(e.type, e.keyCode);
                      keyCode = e.keyCode;
                      if (keyCode === 229) {
                      keyCode = e.target.value.charAt(e.target.selectionStart - 1).charCodeAt();
                          console.log(keyCode);
                      }
                      if (64 <= keyCode && keyCode <= 95) {
                          input = String.fromCharCode(keyCode - 64);
                      } else if (97 <= keyCode && keyCode <= 122) {
                          input = String.fromCharCode(keyCode - 96);
                      } else {
                          return true;
                      }
                      if (e.type === 'keyup') {
                          window.term.input(input);
                          e.target.value = e.target.value.slice(0, -1);
                          window.ctrl = false;
                      }
                      return false;
                  } else {
                      return true;
                  }
              });
            })();
            """;
    public static final String ENABLE_CTRL_KEY = "javascript:(function(){window.ctrl=true;})();";

    public TerminalView(Context context, AttributeSet attrs) {
        super(context, attrs);
    }
}
