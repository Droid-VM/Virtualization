#define LOG_TAG "VirtualizationService"

#include <android-base/unique_fd.h>
#include <android/binder_ibinder_jni.h>
#include <errno.h>
#include <jni.h>
#include <log/log.h>
#include <poll.h>

#include <binder_rpc_unstable.hpp>
#include <string>

using namespace android::base;

// Wrapper around ARpcSession handle that automatically frees the handle when
// it goes out of scope.
class RpcSessionHandle {
public:
    RpcSessionHandle() : mHandle(ARpcSession_new()) {}
    ~RpcSessionHandle() { ARpcSession_free(mHandle); }

    ARpcSession* get() { return mHandle; }

private:
    ARpcSession* mHandle;
};

extern "C" int connect_rpc();

extern "C" JNIEXPORT jobject JNICALL Java_com_ferrochrome_javaclient_MainActivity_nativeConnect(
        JNIEnv* env, [[maybe_unused]] jclass clazz) {
    int clientFd = connect_rpc();

    RpcSessionHandle session;
    ARpcSession_setFileDescriptorTransportMode(session.get(),
                                               ARpcSession_FileDescriptorTransportMode::Unix);
    ARpcSession_setMaxIncomingThreads(session.get(), 2);
    // SAFETY - ARpcSession_setupUnixDomainBootstrapClient does not take ownership of clientFd.
    auto client = ARpcSession_setupUnixDomainBootstrapClient(session.get(), clientFd);
    return AIBinder_toJavaBinder(env, client);
}
