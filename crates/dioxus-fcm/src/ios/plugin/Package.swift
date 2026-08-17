// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "FcmPlugin",
    platforms: [
        .iOS(.v14)
    ],
    products: [
        .library(
            name: "FcmPlugin",
            type: .static,
            targets: ["FcmPlugin"]
        )
    ],
    dependencies: [
        .package(url: "https://github.com/firebase/firebase-ios-sdk", from: "11.0.0")
    ],
    targets: [
        .target(
            name: "FcmPlugin",
            dependencies: [
                .product(name: "FirebaseMessaging", package: "firebase-ios-sdk")
            ],
            path: "Sources",
            resources: [
                .copy("Resources")
            ],
            linkerSettings: [
                .linkedFramework("Foundation"),
                .linkedFramework("UIKit"),
                .linkedFramework("UserNotifications")
            ]
        )
    ]
)
