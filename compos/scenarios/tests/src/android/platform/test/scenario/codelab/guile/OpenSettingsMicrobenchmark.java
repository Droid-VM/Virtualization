package android.platform.test.scenario.codelab.guile;

import android.platform.test.microbenchmark.Microbenchmark;
import android.platform.test.rule.DropCachesRule;
import android.platform.test.rule.KillAppsRule;
import android.platform.test.rule.PressHomeRule;

import org.junit.Rule;
import org.junit.rules.RuleChain;
import org.junit.runner.RunWith;

@RunWith(Microbenchmark.class)
public class OpenSettingsMicrobenchmark extends OpenSettings {
    @Rule
    public RuleChain rules =
            RuleChain.outerRule(new KillAppsRule("com.android.settings"))
                    .around(new DropCachesRule())
                    .around(new PressHomeRule());
}