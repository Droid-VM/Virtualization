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

package android.system.virtualmachine;

import android.system.ErrnoException;
import android.system.Os;
import android.system.OsConstants;
import android.system.StructPollfd;
import android.util.Log;

import libcore.io.IoUtils;

import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;

/**
 * Multiplex the virtual machine console and host pseudo terminal.
 *
 * @hide
 */
class ConsoleForwarder implements Runnable {
    private static final String TAG = ConsoleForwarder.class.getSimpleName();
    private static final int READ_BUFFER_SIZE = 4 * 1024;

    private final FileInputStream mConsoleOutput;
    private final OutputStream mConsoleInput;
    private final FileInputStream mPtyInput;
    private final OutputStream mPtyOutput;
    private final FileDescriptor mConsoleFd;
    private final FileDescriptor mPtyFd;
    private final File mLogFile;
    private final byte[] mReadBuffer = new byte[READ_BUFFER_SIZE];

    ConsoleForwarder(
            FileInputStream consoleOutput,
            OutputStream consoleInput,
            FileDescriptor pty,
            File logFile)
            throws IOException {
        mConsoleOutput = consoleOutput;
        mConsoleInput = consoleInput;
        mPtyInput = new FileInputStream(pty);
        mPtyOutput = new FileOutputStream(pty);
        mConsoleFd = mConsoleOutput.getFD();
        mPtyFd = pty;
        mLogFile = logFile;
    }

    @Override
    public void run() {
        OutputStream logFileWriter = null;
        try {
            List<PollEntry> pollSet = new ArrayList<>(2);
            List<OutputStream> forwardTargets = new ArrayList<>(List.of(mPtyOutput));
            try {
                logFileWriter = new LineBufferedOutputStream(new FileOutputStream(mLogFile));
                forwardTargets.add(logFileWriter);
                Log.d(TAG, "Console log file: " + mLogFile);
            } catch (Exception e) {
                Log.d(TAG, "Failed to open log file: " + mLogFile, e);
            }
            pollSet.add(new PollEntry("vm console", mConsoleFd, mConsoleOutput, forwardTargets));
            pollSet.add(new PollEntry("host console", mPtyFd, mPtyInput, List.of(mConsoleInput)));

            StructPollfd[] pollfds = getPollfds(pollSet);
            while (!Thread.interrupted()) {
                if (pollSet.isEmpty()) {
                    break;
                }
                try {
                    if (Os.poll(pollfds, -1) < 0) {
                        break;
                    }
                } catch (ErrnoException e) {
                    Log.d(TAG, "Failed to poll fds", e);
                    break;
                }
                // Remove fd from the poll set if any error.
                if (pollSet.removeIf(entry -> !doForward(entry))) {
                    pollfds = getPollfds(pollSet);
                }
            }

        } catch (Exception e) {
            Log.d(TAG, "Exit thread with error", e);
        } finally {
            IoUtils.closeQuietly(logFileWriter);
            Log.d(TAG, "Exit thread");
        }
    }

    private boolean doForward(PollEntry entry) {
        final String fdName = entry.mName;
        final StructPollfd pollfd = entry.mPollfd;
        int len;
        if ((pollfd.revents & (OsConstants.POLLERR | OsConstants.POLLHUP)) != 0) {
            Log.d(TAG, "poll " + fdName + " error, revents: " + pollfd.revents);
            return false;
        }
        if ((pollfd.revents & OsConstants.POLLIN) != 0) {
            try {
                len = entry.mIn.read(mReadBuffer);
            } catch (IOException e) {
                Log.d(TAG, "Failed to read " + fdName, e);
                return false;
            }
            if (len < 0) {
                Log.d(TAG, "EOF reached while reading " + fdName);
                return false;
            }
            if (len > 0) {
                // Failing to write to destination is _not_ fatal.
                entry.mOut.removeIf(
                        out -> {
                            try {
                                out.write(mReadBuffer, 0, len);
                            } catch (Exception e) {
                                Log.d(TAG, "Failed to post " + fdName, e);
                                return true;
                            }
                            return false;
                        });
            }
        }
        return true;
    }

    private static StructPollfd[] getPollfds(Collection<PollEntry> pollSet) {
        return pollSet.stream().map(p -> p.mPollfd).toArray(StructPollfd[]::new);
    }

    private static class PollEntry {
        public final String mName;
        public final StructPollfd mPollfd;
        public final InputStream mIn;
        public final List<OutputStream> mOut;

        PollEntry(
                String name, FileDescriptor fd, FileInputStream in, Collection<OutputStream> out) {
            mName = name;
            mPollfd = new StructPollfd();
            mPollfd.fd = fd;
            mPollfd.events = (short) OsConstants.POLLIN;
            mIn = in;
            mOut = new ArrayList<>(out);
        }
    }

    private static class LineBufferedOutputStream extends BufferedOutputStream {
        LineBufferedOutputStream(OutputStream out) {
            super(out);
        }

        @Override
        public void write(byte[] buf, int off, int len) throws IOException {
            super.write(buf, off, len);
            for (int i = 0; i < len; ++i) {
                if (buf[off + i] == '\n') {
                    flush();
                    break;
                }
            }
        }
    }
}
