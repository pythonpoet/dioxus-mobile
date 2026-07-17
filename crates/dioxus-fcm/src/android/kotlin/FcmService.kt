//package {{package}}
package dev.dioxus.main // <--- Replace this with your actual package if it's different

import android.util.Log
import android.content.Context
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import com.google.android.gms.tasks.Task

class FcmService : FirebaseMessagingService() {

    companion object {
        private const val TAG = "FcmService"

        @JvmStatic
        fun kotlinAvailable(): String = "inshallah"

        @JvmStatic
        fun requestToken(context: Context) {
            FirebaseMessaging.getInstance().token
                .addOnCompleteListener { task ->
                    if (task.isSuccessful) {
                        nativeOnToken(task.result)
                    } else {
                        nativeOnError(task.exception?.message ?: "unknown error")
                    }
                }
        }

        @JvmStatic
        fun init(context: Context) {
            // Firebase registers automatically. You usually don't need to call a register() method.
            Log.d(TAG, "FCM Initialized")
        }
    }

    override fun onNewToken(token: String) {
        nativeOnToken(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {}
}

// Top-level external functions
private external fun nativeOnToken(token: String)
private external fun nativeOnError(msg: String)
