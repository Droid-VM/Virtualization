# Installation
```sh
. build_and_run.sh
```

.. will install necessary apps and rust binary to /data/local/tmp/rustbackend

# How to use
  - Launch Java client, and click buttons.
  - There's no way to disconnect gracefully. Just shutdown and relaunch java client.
  - Grab these logs: JavaClient Instance ServerSelfInstance ServerKernelInstance ServerRpcInstance JavaBackend

# Note
Just FYI, you can't send a socket binder over kernel binder. (opposite may be OK)

03-18 23:55:55.320  8273  8273 I RustBackend-RPC: rustbackend_rpc: Started RpcServer. Ready to accept connections
03-18 23:55:55.324  8208  8208 E Parcel  : Sending a socket binder over kernel binder is prohibited
03-18 23:55:55.324  8208  8208 I RpcState: RpcState has no binders left, so triggering shutdown...
03-18 23:55:55.324  8273  8273 I RustBackend-RPC: rustbackend_rpc: (unreachable) Shutting down RPC server
03-18 23:55:55.325  8243  8243 D AndroidRuntime: Shutting down VM
--------- beginning of crash
03-18 23:55:55.326  8243  8243 E AndroidRuntime: FATAL EXCEPTION: main
03-18 23:55:55.326  8243  8243 E AndroidRuntime: Process: com.ferrochrome.javaclient, PID: 8243
03-18 23:55:55.326  8243  8243 E AndroidRuntime: java.lang.UnsupportedOperationException
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.os.BinderProxy.transactNative(Native Method)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.os.BinderProxy.transact(BinderProxy.java:592)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.ferrochrome.IService$Stub$Proxy.create(IService.java:128)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.ferrochrome.javaclient.MainActivity.handleConnect(MainActivity.java:120)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.ferrochrome.javaclient.MainActivity.connectToNativeService(MainActivity.java:190)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.ferrochrome.javaclient.MainActivity.lambda$onCreate$2(MainActivity.java:92)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.ferrochrome.javaclient.MainActivity.$r8$lambda$MuMAW0QKhzmimJqIz6ymqhWcXSQ(MainActivity.java:0)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.ferrochrome.javaclient.MainActivity$$ExternalSyntheticLambda2.onClick(R8$$SyntheticClass:0)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.view.View.performClick(View.java:8083)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.google.android.material.button.MaterialButton.performClick(MaterialButton.java:1305)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.view.View.performClickInternal(View.java:8060)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.view.View.-$$Nest$mperformClickInternal(Unknown Source:0)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.view.View$PerformClick.run(View.java:31532)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.os.Handler.handleCallback(Handler.java:995)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.os.Handler.dispatchMessage(Handler.java:103)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.os.Looper.loopOnce(Looper.java:248)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.os.Looper.loop(Looper.java:338)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at android.app.ActivityThread.main(ActivityThread.java:8994)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at java.lang.reflect.Method.invoke(Native Method)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.android.internal.os.RuntimeInit$MethodAndArgsCaller.run(RuntimeInit.java:593)
03-18 23:55:55.326  8243  8243 E AndroidRuntime: 	at com.android.internal.os.ZygoteInit.main(ZygoteInit.java:932)

