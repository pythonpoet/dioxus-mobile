//package {{package}}
package dev.dioxus.main

import android.util.Log
import android.content.Context
import android.content.pm.PackageManager
import android.app.Activity
import android.os.Build

import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.app.NotificationManagerCompat

import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class FcmService : FirebaseMessagingService() {

    override fun onNewToken(token: String) {
        nativeOnToken(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {}

    companion object {
        private const val TAG = "FcmService"
        private const val POST_NOTIFICATIONS = "android.permission.POST_NOTIFICATIONS"
        private const val REQUEST_CODE = 1001

        @JvmStatic
        fun kotlinAvailable(): String = "inshallah"

        @JvmStatic
        fun requestToken(context: Context) {
            try {
                FirebaseMessaging.getInstance().token
                    .addOnCompleteListener { task ->
                        if (task.isSuccessful) {
                            nativeOnToken(task.result)
                        } else {
                            nativeOnError(task.exception?.message ?: "unknown error")
                        }
                    }
            } catch (e: Exception) {
                nativeOnError(e.message ?: "Firebase not initialized")
            }
        }

        @JvmStatic
        fun init(context: Context) {
            com.google.firebase.FirebaseApp.initializeApp(context)
            Log.d(TAG, "FCM Initialized")
        }

        @JvmStatic
        fun requestNotificationPermission(activity: Activity) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                val granted = ContextCompat.checkSelfPermission(activity, POST_NOTIFICATIONS) ==
                        PackageManager.PERMISSION_GRANTED
                if (granted) {
                    nativeOnPermissionResult(true)
                } else {
                    ActivityCompat.requestPermissions(
                        activity, arrayOf(POST_NOTIFICATIONS), REQUEST_CODE
                    )
                }
            } else {
                nativeOnPermissionResult(true)
            }
        }


        @JvmStatic
        fun notificationsEnabled(context: Context): Boolean =
            NotificationManagerCompat.from(context).areNotificationsEnabled()

            @JvmStatic
            fun handlePermissionResult(requestCode: Int, grantResults: IntArray): Boolean {
                if (requestCode != REQUEST_CODE) return false
                val granted = grantResults.isNotEmpty() &&
                        grantResults[0] == PackageManager.PERMISSION_GRANTED
                nativeOnPermissionResult(granted)
                return true
            }

        @JvmStatic
        external fun nativeOnToken(token: String)

        @JvmStatic
        external fun nativeOnError(msg: String)

        @JvmStatic
        external fun nativeOnPermissionResult(granted: Boolean)
    }
}
