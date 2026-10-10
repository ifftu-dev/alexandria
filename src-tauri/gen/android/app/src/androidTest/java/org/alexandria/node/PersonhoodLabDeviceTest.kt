package org.alexandria.node

import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class PersonhoodLabDeviceTest {
  private fun state(): JSONObject = JSONObject(MainActivity.personhoodLab("status"))
  private fun action(value: String): JSONObject = JSONObject(MainActivity.personhoodLab(value))

  private fun waitFor(description: String, timeout: Long = 30000, condition: (JSONObject) -> Boolean): JSONObject {
    val deadline = System.nanoTime() + timeout * 1000000
    var last = state()
    while (System.nanoTime() < deadline) {
      last = state()
      if (condition(last)) return last
      if (last.getString("phase") == "error") fail("$description: $last")
      Thread.sleep(40)
    }
    throw AssertionError("Timed out $description: $last")
  }

  @Test fun syntheticDownloadResumeAndLifecycle() {
    check(InstrumentationRegistry.getArguments().getString("waitForActivitiesToComplete") == "false") {
      "Use -e waitForActivitiesToComplete false: closing Tauri's last Activity exits the instrumentation process"
    }
    val evidence = JSONArray()
    // The runner must report assertions before Android terminates the app process.
    // Closing the last Tauri Activity here exits the host before the runner reports.
    ActivityScenario.launch(MainActivity::class.java).let { scenario ->
      waitFor("developer build ready") { it.getBoolean("enabled") }
      action("cancel")
      waitFor("previous work stopped") { it.getString("phase") in listOf("idle", "cancelled", "complete", "error") }
      action("remove_key")
      assertEquals("missing", state().getString("key_status"))
      evidence.put(JSONObject().put("check", "remove_key").put("passed", true))

      action("download")
      waitFor("download partial bytes", 120000) { it.getLong("downloaded_bytes") > 8 * 1024 * 1024 }
      action("cancel")
      val cancelled = waitFor("download cancelled") { it.getString("phase") == "cancelled" }
      val retained = cancelled.getLong("downloaded_bytes")
      assertTrue(retained > 0 && retained < PersonhoodKeyTransfer.SIZE)
      assertEquals("partial", cancelled.getString("key_status"))
      evidence.put(JSONObject().put("check", "cancel_download_retains_partial").put("bytes", retained))

      scenario.recreate()
      assertEquals(retained, state().getLong("downloaded_bytes"))
      action("download")
      val downloaded = waitFor("resume and verify full key", 180000) { it.getString("phase") == "complete" }
      assertEquals(PersonhoodKeyTransfer.SIZE, downloaded.getLong("downloaded_bytes"))
      assertEquals("stored", downloaded.getString("key_status"))
      evidence.put(JSONObject().put("check", "resume_after_activity_recreation").put("state", downloaded))

      action("prove")
      waitFor("proving before cancel") { it.getString("phase") == "proving" }
      action("cancel")
      evidence.put(JSONObject().put("check", "cancel_proving").put("state",
        waitFor("proof cancelled") { it.getString("phase") == "cancelled" }))

      action("prove")
      waitFor("proving before background") { it.getString("phase") == "proving" }
      scenario.moveToState(Lifecycle.State.CREATED)
      evidence.put(JSONObject().put("check", "background_cancels").put("state",
        waitFor("background cancellation") { it.getString("phase") == "cancelled" }))
      scenario.moveToState(Lifecycle.State.RESUMED)

      action("prove")
      waitFor("proving before simulated memory warning") { it.getString("phase") == "proving" }
      scenario.onActivity { it.onTrimMemory(android.content.ComponentCallbacks2.TRIM_MEMORY_RUNNING_CRITICAL) }
      evidence.put(JSONObject().put("check", "simulated_memory_warning").put("state",
        waitFor("memory cancellation") { it.getString("phase") == "cancelled" }))

      repeat(2) {
        action("prove")
        val completed = waitFor("synthetic proof verified") { it.getString("phase") == "complete" }
        assertTrue(completed.getJSONObject("result").getLong("peak_rss_bytes") > 0)
        evidence.put(JSONObject().put("check", "proof_verified_$it").put("state", completed))
      }
      val context = InstrumentationRegistry.getInstrumentation().targetContext
      File(context.filesDir, "personhood-lab-device-tests.json").writeText(evidence.toString(2))
      scenario.moveToState(Lifecycle.State.CREATED)
    }
  }
}
