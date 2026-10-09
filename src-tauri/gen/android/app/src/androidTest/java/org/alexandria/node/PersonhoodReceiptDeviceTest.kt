package org.alexandria.node

import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class PersonhoodReceiptDeviceTest {
  private fun webView(view: View): WebView? {
    if (view is WebView) return view
    if (view is ViewGroup) for (index in 0 until view.childCount) {
      webView(view.getChildAt(index))?.let { return it }
    }
    return null
  }

  private fun evaluate(scenario: ActivityScenario<MainActivity>, script: String): String {
    val latch = CountDownLatch(1)
    var result = "null"
    scenario.onActivity { activity ->
      activity.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      checkNotNull(webView(activity.window.decorView)).evaluateJavascript(script) {
        result = it
        latch.countDown()
      }
    }
    check(latch.await(10, TimeUnit.SECONDS)) { "WebView evaluation timed out" }
    return result
  }

  @Test fun privateReceiptAccountSessionAndReplay() {
    val arguments = InstrumentationRegistry.getArguments()
    check(arguments.getString("waitForActivitiesToComplete") == "false")
    val network = checkNotNull(arguments.getString("networkId")) { "Pass the embedded network_id as -e networkId" }
    check(network.matches(Regex("[a-zA-Z0-9_-]{1,64}")))
    val scenario = ActivityScenario.launch(MainActivity::class.java)
    val readyDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(30)
    while (evaluate(scenario, "Boolean(window.__TAURI_INTERNALS__ && document.querySelector('[data-testid=personhood-lab]'))") != "true") {
      check(System.nanoTime() < readyDeadline) { "Lab WebView did not become ready" }
      Thread.sleep(100)
    }
    evaluate(scenario, """
      window.__personhoodReceiptTest = null;
      (async () => {
        const rpc = (command, args = {}, session) => window.__TAURI_INTERNALS__.invoke(command, args,
          session ? { headers: { 'x-alexandria-profile-session': session } } : {});
        const assert = (condition, message) => { if (!condition) throw new Error(message); };
        const rejected = async (fn, message) => { let failed = false; try { await fn(); } catch { failed = true; } assert(failed, message); };
        const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
        const password = 'SyntheticReceiptTestOnly!42';
        const profiles = [];
        const checks = [];
        try {
          assert(!(await rpc('get_active_profile_id')), 'Lock any existing profile before this test');
          assert((await rpc('personhood_lab_status')).key_status === 'stored', 'Download the synthetic key first');
          const create = async suffix => {
            const value = await rpc('create_profile', { username: 'receipt' + suffix + Date.now().toString().slice(-6),
              displayName: 'Synthetic receipt test ' + suffix, networkId: '$network', password,
              avatar: null, roles: ['learner'], birthdate: '2000-01-01' });
            profiles.push(value.summary.id);
            return value.summary.id;
          };
          const first = await create('a');
          let session = await rpc('get_profile_session_token');
          const before = await rpc('get_profile', {}, session);
          const challengeId = await rpc('personhood_receipt_prepare', {}, session);
          const receipt = await rpc('personhood_receipt_prove', { challengeId }, session);
          assert(receipt.kind === 'synthetic_diagnostic' && receipt.id === challengeId, 'Wrong receipt');
          assert(JSON.stringify(before) === JSON.stringify(await rpc('get_profile', {}, session)), 'Receipt changed account identity or permissions');
          assert(JSON.stringify(await rpc('personhood_receipt_prove', { challengeId }, session)) === JSON.stringify(receipt), 'Retry changed receipt');
          assert((await rpc('personhood_receipt_list', {}, session)).length === 1, 'Retry created another receipt');
          checks.push('bound_receipt', 'account_unchanged', 'idempotent_retry');
          let cancelledId = await rpc('personhood_receipt_prepare', {}, session);
          await rpc('personhood_receipt_cancel', { challengeId: cancelledId }, session);
          await rejected(() => rpc('personhood_receipt_prove', { challengeId: cancelledId }, session), 'Cancelled challenge accepted');
          checks.push('cancel_before_start');
          const oldId = await rpc('personhood_receipt_prepare', {}, session);
          const oldSession = session;
          await rpc('lock_profile');
          await rpc('unlock_profile', { id: first, password });
          session = await rpc('get_profile_session_token');
          await rejected(() => rpc('personhood_receipt_list', {}, oldSession), 'Old IPC session accepted');
          await rejected(() => rpc('personhood_receipt_prove', { challengeId: oldId }, session), 'Old unlock challenge accepted');
          assert((await rpc('personhood_receipt_list', {}, session)).length === 1, 'Receipt did not persist');
          checks.push('receipt_persists_after_unlock', 'old_session_rejected');
          const interruptedId = await rpc('personhood_receipt_prepare', {}, session);
          const pending = rpc('personhood_receipt_prove', { challengeId: interruptedId }, session).then(() => true, () => false);
          for (let i = 0; i < 100; i++) {
            if ((await rpc('personhood_lab_status')).phase === 'proving') break;
            await delay(50);
          }
          await rpc('lock_profile');
          assert(!(await pending), 'Profile lock accepted an in-flight receipt');
          await rpc('unlock_profile', { id: first, password });
          session = await rpc('get_profile_session_token');
          assert((await rpc('personhood_receipt_list', {}, session)).length === 1, 'Interrupted proof stored a receipt');
          checks.push('lock_interrupts_proof');
          await rpc('lock_profile');
          await create('b');
          session = await rpc('get_profile_session_token');
          assert((await rpc('personhood_receipt_list', {}, session)).length === 0, 'Cross-profile receipt leak');
          await rejected(() => rpc('personhood_receipt_prove', { challengeId }, session), 'Cross-account proof reuse accepted');
          checks.push('cross_profile_isolation');
          window.__personhoodReceiptTest = { passed: true, checks, receipt };
        } catch (error) {
          window.__personhoodReceiptTest = { passed: false, checks, error: String(error) };
        } finally {
          if (profiles.length) {
            try {
              await rpc('lock_profile');
              for (const id of profiles) await rpc('delete_profile', { id, password });
            } catch (error) {
              window.__personhoodReceiptTest = { passed: false, checks, error: 'Cleanup: ' + String(error) };
            }
          }
          window.__personhoodReceiptTest.finished = true;
        }
      })();
    """.trimIndent())
    val deadline = System.nanoTime() + TimeUnit.MINUTES.toNanos(3)
    var evidence = JSONObject()
    while (System.nanoTime() < deadline) {
      val result = evaluate(scenario, "window.__personhoodReceiptTest")
      if (result != "null") {
        evidence = JSONObject(result)
        if (evidence.optBoolean("finished")) break
      }
      Thread.sleep(100)
    }
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    File(context.filesDir, "personhood-receipt-device-tests.json").writeText(evidence.toString(2))
    assertTrue(evidence.toString(), evidence.optBoolean("finished") && evidence.optBoolean("passed"))
    assertFalse("Receipt worker retained proof or witness files", File(context.filesDir, "personhood-lab/receipt-run").exists())
    scenario.moveToState(Lifecycle.State.CREATED)
  }
}
