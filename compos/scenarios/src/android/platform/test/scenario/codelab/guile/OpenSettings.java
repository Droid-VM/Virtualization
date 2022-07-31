package android.platform.test.scenario.codelab.guile;

import android.os.SystemClock;
import android.platform.test.scenario.annotation.Scenario;
import android.support.test.uiautomator.UiDevice;
import androidx.test.InstrumentationRegistry;

import org.junit.AfterClass;
import org.junit.ClassRule;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.JUnit4;

import java.io.IOException;
import java.util.concurrent.TimeUnit;

@Scenario // Marks the test as a CUJ that CrystalBall recognizes.
@RunWith(JUnit4.class)
public class OpenSettings {

    // Lazily initialized UiDevice instance for running shell commands.
    private static UiDevice mDevice;

    @Test
    public void testOpenSettings() throws IOException {
        // Open Settings with a shell command. We usually do something more
        // sophisticated/robust than this, but this example shows that any kind
        // of test code can go here.
        getUiDevice()
                .executeShellCommand("am start com.android.settings");
        SystemClock.sleep(TimeUnit.SECONDS.toMillis(5));
    }

    @AfterClass
    public static void closeApp() throws IOException {
        // Press home to go back to the home screen. We usually do something
        // more robust like backing to the homescreen with the "Back" button,
        // but this is a simplistic example.
        getUiDevice().pressHome();
    }

    private static UiDevice getUiDevice() {
        if (mDevice == null) {
            mDevice = UiDevice.getInstance(
                    InstrumentationRegistry.getInstrumentation());
        }
        return mDevice;
    }
}