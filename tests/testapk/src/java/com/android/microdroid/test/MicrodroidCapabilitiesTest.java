package com.android.microdroid.test;

import static com.google.common.truth.Truth.assertWithMessage;

import android.system.virtualmachine.VirtualMachineManager;

import com.android.compatibility.common.util.CddTest;
import com.android.microdroid.test.device.MicrodroidDeviceTestBase;

import org.junit.Test;

/**
 * Test the advertised AVF capabilities include the ability to start some type of VM.
 *
 * <p>Tests in MicrodroidTests run on either protected or non-protected VMs, provided they are
 * supported. If neither is they are all skipped. So we need a separate test (that doesn't call
 * {@link #prepareTestSetup}) to make sure that at least one of these is available.
 */
public class MicrodroidCapabilitiesTest extends MicrodroidDeviceTestBase {
    @Test
    @CddTest(requirements = {"9.17/C-1-1", "9.17/C-2-1"})
    public void supportForProtectedOrNonProtectedVms() {
        assumeSupportedDevice();

        // (There's a test for devices that don't expose the system feature over in
        // NoMicrodroidTest.)
        assumeFeatureVirtualizationFramework();

        int capabilities = getVirtualMachineManager().getCapabilities();
        int vmCapabilities =
                capabilities
                        & (VirtualMachineManager.CAPABILITY_PROTECTED_VM
                                | VirtualMachineManager.CAPABILITY_NON_PROTECTED_VM);
        assertWithMessage(
                        "A device that has FEATURE_VIRTUALIZATION_FRAMEWORK must support at least"
                                + " one of protected or non-protected VMs")
                .that(vmCapabilities)
                .isNotEqualTo(0);
    }
}
