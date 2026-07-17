// package {{package}}
package dev.dioxus.main


//import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity

//import kotlin.jvm.JvmStatict

class MainActivity : AppCompatActivity() {

    companion object {
        // Keep a weak/static reference so Rust can trigger it via JNI
        private var instance: MainActivity? = null


    }

    // 1. SET UP FIRST in the Activity
    // private val requestPermissionLauncher = registerForActivityResult(
    //     ActivityResultContracts.RequestPermission()
    // ) { isGranted: Boolean ->
    //     if (isGranted) {
    //         // Permission granted, you could call a native Rust function here if you want to notify Rust
    //     }
    // }
}
