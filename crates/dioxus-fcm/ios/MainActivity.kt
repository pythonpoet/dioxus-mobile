package {{package}}

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity // Or your specific Dioxus Activity base
import androidx.core.content.ContextCompat

class MainActivity : AppCompatActivity() {

    companion object {
        // Keep a weak/static reference so Rust can trigger it via JNI
        private var instance: MainActivity? = null

        @JvmStatic
        fun triggerNotificationPermission() {
            instance?.askNotificationPermission()
        }
    }

    // 1. SET UP FIRST in the Activity
    private val requestPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { isGranted: Boolean ->
        if (isGranted) {
            // Permission granted, you could call a native Rust function here if you want to notify Rust
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        instance = this
        // Dioxus typically handles setContentView, so leave it to the framework
    }

    override fun onDestroy() {
        super.onDestroy()
        if (instance == this) instance = null
    }

    private fun askNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS)
                == PackageManager.PERMISSION_GRANTED
            ) {
                // Already granted
            } else if (shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS)) {
                // Show rationale if needed
                requestPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            } else {
                requestPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }
}
