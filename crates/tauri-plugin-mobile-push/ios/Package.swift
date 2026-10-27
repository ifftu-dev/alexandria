// swift-tools-version:5.5

import PackageDescription

let package = Package(
  name: "tauri-plugin-mobile-push",
  platforms: [
    // Firebase 13 supports iOS 15 and later; the app's own deployment target
    // is higher still.
    .iOS(.v15),
    // Never built for macOS, but SwiftPM checks every platform: without
    // this the package defaults to macOS 10.13, below FirebaseMessaging's 11.
    .macOS(.v11),
  ],
  products: [
    .library(
      name: "tauri-plugin-mobile-push",
      type: .static,
      targets: ["tauri-plugin-mobile-push"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api"),
    // Alexandria modification: the token this plugin hands out is an FCM
    // registration token on iOS as well as Android, so one sender on the
    // server reaches both. FirebaseMessaging trades the APNs token for it.
    // Only the Messaging product is linked; SwiftPM still resolves the whole
    // firebase-ios-sdk graph on first build (slow, once).
    .package(
      name: "Firebase",
      url: "https://github.com/firebase/firebase-ios-sdk.git",
      .upToNextMajor(from: "13.0.0")),
  ],
  targets: [
    .target(
      name: "tauri-plugin-mobile-push",
      dependencies: [
        .byName(name: "Tauri"),
        .product(name: "FirebaseMessaging", package: "Firebase"),
      ],
      path: "Sources")
  ]
)
