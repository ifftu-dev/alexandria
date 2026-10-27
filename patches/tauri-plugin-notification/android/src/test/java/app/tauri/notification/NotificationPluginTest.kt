// Alexandria regression: real native launch intent and listener command boundary.
package app.tauri.notification

import android.app.Activity
import android.content.Intent
import android.webkit.WebView
import app.tauri.plugin.*
import com.fasterxml.jackson.core.JsonParser
import com.fasterxml.jackson.databind.*
import com.fasterxml.jackson.databind.module.SimpleModule
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], manifest = Config.NONE)
class NotificationPluginTest {
    @Test
    fun coldLocalTapReachesLaterNativeSubscriberWithRecipientContextOnce() {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        val events = mutableListOf<JSONObject>()
        val mapper = ObjectMapper()
        mapper.registerModule(SimpleModule().addDeserializer(app.tauri.plugin.Channel::class.java,
            object : JsonDeserializer<app.tauri.plugin.Channel>() {
                override fun deserialize(parser: JsonParser, context: DeserializationContext): app.tauri.plugin.Channel {
                    return app.tauri.plugin.Channel(9, { events.add(JSONObject(it)) }, mapper)
                }
            }))
        val plugin = NotificationPlugin(activity)
        val handle = PluginHandle(PluginManager, "notification", plugin, "{}", mapper)
        val extra = JSONObject().put("profileId", "profile-A").put("networkId", "network-A")
            .put("notificationId", "11111111-1111-4111-8111-111111111111")
            .put("route", "/interview-room/secret?cloud=https%3A%2F%2Ftrusted.example")
        activity.intent = Intent(Intent.ACTION_MAIN)
            .putExtra(NOTIFICATION_INTENT_KEY, 42)
            .putExtra(ACTION_INTENT_KEY, "tap")
            .putExtra(NOTIFICATION_OBJ_INTENT_KEY, JSONObject().put("id", 42).put("extra", extra).toString())
        handle.load(WebView(activity))
        assertTrue(events.isEmpty())
        fun subscribe() = plugin.registerListener(Invoke(1, "registerListener", 2, 3,
            { _, _ -> }, """{"event":"actionPerformed","handler":"__CHANNEL__:9"}""", mapper))
        subscribe()
        assertEquals(1, events.size)
        assertEquals("tap", events.single().getString("actionId"))
        assertEquals(extra.toString(), events.single().getJSONObject("notification").getJSONObject("extra").toString())
        subscribe()
        assertEquals(1, events.size)
    }
}
