package android.compos.test.scenario.codelab.guile;

import android.compos.test.scenario.annotation.Scenario;

import org.junit.AfterClass;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.JUnit4;

@Scenario  // Marks the test as a CUJ that CrystalBall recognizes.
@RunWith(JUnit4.class)
public class OpenSettings {
 
    @Test
    public void testOpenSettings() {
        // Code to open Settings will go here.
      try {
        Thread.sleep(10000);
      } catch (InterruptedException e) {
        throw new AssertionError("Thread sleep interrupted", e);
      }
        
    }

    // @AfterClass
    // public void closeApp() {
    //     // Code to exit out of the app, so that the device goes back to its
    //     // initial state, will go here.
    // }
}