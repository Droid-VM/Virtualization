package com.android.virtualization.vmlauncher;

import android.util.Log;

import com.android.virtualization.vmlauncher.proto.DebianServiceGrpc;
import com.android.virtualization.vmlauncher.proto.IpAddr;
import com.android.virtualization.vmlauncher.proto.ReportVmIpAddrResponse;

import io.grpc.stub.StreamObserver;

class DebianServiceImpl extends DebianServiceGrpc.DebianServiceImplBase {
    public static final String TAG = "DebianService";
    private final DebianServiceCallback mCallback;

    protected DebianServiceImpl(DebianServiceCallback callback) {
        super();
        mCallback = callback;
    }

    @Override
    public void reportVmIpAddr(
            IpAddr request, StreamObserver<ReportVmIpAddrResponse> responseObserver) {
        Log.d(DebianServiceImpl.TAG, "reportVmIpAddr: " + request.toString());
        mCallback.onIpAddressAvailable(request.getAddr());
        ReportVmIpAddrResponse reply = ReportVmIpAddrResponse.newBuilder().setSuccess(true).build();
        responseObserver.onNext(reply);
        responseObserver.onCompleted();
    }

    protected interface DebianServiceCallback {
        void onIpAddressAvailable(String ipAddr);
    }
}
