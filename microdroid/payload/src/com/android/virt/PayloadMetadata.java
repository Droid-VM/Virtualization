package com.android.virt;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;

/** Provides utility to create/read/write PayloadMetadata */
public class PayloadMetadata {
    public static void write(PayloadMetadataProtos.Metadata metadata, File file)
            throws IOException {
        byte[] message = metadata.toByteArray();

        DataOutputStream os = new DataOutputStream(new FileOutputStream(file));
        // write length prefix (4-byte, big-endian)
        os.writeInt(message.length);
        // write the message
        os.write(message);
    }

    public static PayloadMetadataProtos.Metadata read(File file) throws IOException {
        long fileSize = file.length();

        DataInputStream is = new DataInputStream(new FileInputStream(file));
        // read length prefix (4-byte, big-endian)
        int messageSize = is.readInt();
        if (messageSize + 4 /* size of int */ != fileSize) {
            throw new IOException(
                    String.format(
                            "Invalid metadata: size(%d) " + "doesn't match with content size(%d)",
                            messageSize, fileSize - 4));
        }
        // read the message
        return PayloadMetadataProtos.Metadata.parseFrom(is);
    }

    public static PayloadMetadataProtos.Metadata metadata(
            String configPath,
            PayloadMetadataProtos.ApkPayload apk,
            Iterable<? extends PayloadMetadataProtos.ApexPayload> apexes) {
        return PayloadMetadataProtos.Metadata.newBuilder()
                .setVersion(1)
                .setConfigPath(configPath)
                .setApk(apk)
                .addAllApexes(apexes)
                .build();
    }

    public static PayloadMetadataProtos.ApkPayload apk(String name) {
        return PayloadMetadataProtos.ApkPayload.newBuilder()
                .setName(name)
                .setPayloadPartitionName("microdroid-apk")
                .setIdsigPartitionName("microdroid-apk-idsig")
                .build();
    }

    public static PayloadMetadataProtos.ApexPayload apex(String name) {
        return PayloadMetadataProtos.ApexPayload.newBuilder()
                .setName(name)
                .setIsFactory(true)
                .setPartitionName(name)
                .build();
    }
}
