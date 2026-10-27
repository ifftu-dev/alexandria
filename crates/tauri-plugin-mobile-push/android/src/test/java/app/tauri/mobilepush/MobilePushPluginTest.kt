package app.tauri.mobilepush

import android.app.Activity
import app.tauri.plugin.Invoke
import com.fasterxml.jackson.databind.ObjectMapper
import com.google.firebase.FirebaseApp
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], manifest = Config.NONE)
class MobilePushPluginTest {
    @Test
    fun missingFirebaseRejectsTokenCommandWithoutCrashing() {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        FirebaseApp.getApps(activity).forEach { it.delete() }
        val responses = mutableListOf<Pair<Long, String>>()
        val invoke = Invoke(1, "getToken", 2, 3,
            { callback, data -> responses.add(callback to data) }, "{}", ObjectMapper())

        MobilePushPlugin(activity).getToken(invoke)

        assertEquals(1, responses.size)
        assertEquals(3L, responses.single().first)
        val message = JSONObject(responses.single().second).getString("message")
        assertTrue(message.contains("Push unavailable"))
        assertTrue(message.contains("google-services.json"))
    }
}
