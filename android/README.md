# tunante-android

Build: `./build.sh` (see `CLAUDE.md`). Design and decisions:
[`docs/plan-android.md`](../docs/plan-android.md).

## The release signing key is not in this repository

It lives on lumina, outside the tree:

```
~/.android/tunante-release.jks     PKCS12, RSA 4096, alias "tunante"
~/.android/tunante-release.pass    its password
```

SHA-256 `04:ee:b7:3d:e2:ce:10:eb:9c:a3:bb:57:ab:05:7a:c8:aa:14:55:3c:4d:ba:28:f3:43:6c:38:5d:d5:5c:52:79`

**Back it up.** Android identifies an app by its certificate, not by its package
name, so if that file is lost no future build can ever be installed over an
existing one — the only way through is to uninstall, losing the scanned library.
There is no way to rotate it outside Google Play.

CI reads it from the `ANDROID_KEYSTORE_BASE64` and `ANDROID_KEYSTORE_PASSWORD`
secrets. Without them the build still works, but produces a debug APK: that one
installs and simply cannot be upgraded over, whereas an *unsigned* release APK
would not install at all.

A clone without the key builds fine — `app/build.gradle.kts` only wires the
signing config up when the file is actually there.
