/*
 * Copyright (C) 2021 The Android Open Source Project
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

#include <android-base/logging.h>
#include <android/binder_ibinder.h>
#include <binder/RpcServer.h>
#include <binder/RpcSession.h>
#include <stdio.h>

// This shim won't be necessary when the RPC binder API is ready in Rust. Before that, we need to
// get around __ANDROID_VNDK__ guard.
__attribute__((weak)) android::sp<android::IBinder> AIBinder_toPlatformBinder(AIBinder* binder);
__attribute__((weak)) AIBinder* AIBinder_fromPlatformBinder(
        const android::sp<android::IBinder>& binder);

using android::IBinder;
using android::RpcServer;
using android::RpcSession;
using android::sp;

bool RunRpcServer(AIBinder* service, unsigned int port) {
    auto server = RpcServer::make();
    server->iUnderstandThisCodeIsExperimentalAndIWillNotUseItInProduction();
    if (!server->setupVsockServer(port)) {
        LOG(ERROR) << "Failed to set up vsock server with port " << port;
        return false;
    }
    server->setRootObject(AIBinder_toPlatformBinder(service));
    server->join();

    // Another thread calls shutdown. Wait for it to complete.
    (void)server->shutdown();
    return true;
}

AIBinder* RpcClient(unsigned int cid, unsigned int port) {
    auto session = RpcSession::make();
    if (!session->setupVsockClient(cid, port)) {
        LOG(ERROR) << "Failed to set up vsock client with CID " << cid << " and port " << port;
        return nullptr;
    }
    return AIBinder_fromPlatformBinder(session->getRootObject());
}
