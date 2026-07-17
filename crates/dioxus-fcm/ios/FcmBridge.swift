import Foundation
import FirebaseMessaging
import UIKit

@objc class FcmBridge: NSObject, MessagingDelegate {
    static let shared = FcmBridge()

    func configure() {
        Messaging.messaging().delegate = self
        UIApplication.shared.registerForRemoteNotifications()
    }

    // FCM token refresh callback
    func messaging(_ messaging: Messaging,
                   didReceiveRegistrationToken fcmToken: String?) {
        if let token = fcmToken {
            token.withCString { fcm_on_token($0) }
        }
    }
}

// C-callable entry point invoked from Rust
@_cdecl("fcm_request_token")
func fcm_request_token() {
    Messaging.messaging().token { token, error in
        if let token = token {
            token.withCString { fcm_on_token($0) }
        } else if let error = error {
            error.localizedDescription.withCString { fcm_on_error($0) }
        }
    }
}

// Declared in Rust, implemented via #[no_mangle]
@_silgen_name("fcm_on_token")
func fcm_on_token(_ token: UnsafePointer<CChar>)

@_silgen_name("fcm_on_error")
func fcm_on_error(_ msg: UnsafePointer<CChar>)
