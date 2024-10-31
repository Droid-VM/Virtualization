package com.android.virtualization.terminal;

import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.security.KeyPair;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.cert.Certificate;
import java.security.cert.CertificateEncodingException;

public class CertificateUtils {
    private static final String TAG = "CertificateUtils";

    public static KeyStore.PrivateKeyEntry createOrGetKey() {
        try {
            String alias = "ttyd";

            KeyStore ks = KeyStore.getInstance("AndroidKeyStore");
            ks.load(null);

            if (!ks.containsAlias(alias)) {
                KeyPairGenerator kpg = null;
                kpg =
                        KeyPairGenerator.getInstance(
                                KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore");
                kpg.initialize(
                        new KeyGenParameterSpec.Builder(
                                        alias,
                                        KeyProperties.PURPOSE_SIGN | KeyProperties.PURPOSE_VERIFY)
                                .setDigests(
                                        KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA512)
                                .build());

                KeyPair kp = kpg.generateKeyPair();
            }

            return ((KeyStore.PrivateKeyEntry) ks.getEntry(alias, null));
        } catch (Exception e) {
            Log.e(TAG, "cannot generate or get key", e);
        }
        return null;
    }

    public static void writeCertificateToFile(Context context, Certificate cert) {
        String certFileName = "ca.crt";
        File certFile = new File(context.getFilesDir(), certFileName);
        try (FileOutputStream writer = new FileOutputStream(certFile)) {
            String cert_begin = "-----BEGIN CERTIFICATE-----\n";
            String end_cert = "-----END CERTIFICATE-----\n";
            String output =
                    cert_begin
                            + Base64.encodeToString(cert.getEncoded(), Base64.DEFAULT)
                                    .replaceAll("(.{64})", "$1\n")
                            + end_cert;
            writer.write(output.getBytes());
        } catch (IOException | CertificateEncodingException e) {
            Log.d(TAG, "cannot write cert", e);
        }
    }
}
