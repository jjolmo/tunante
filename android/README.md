# tunante-android

Build: `./build.sh` (see `CLAUDE.md`). Design and decisions:
[`docs/plan-android.md`](../docs/plan-android.md).

## The release signing key is not in this repository

It lives on lumina, outside the tree:

```
~/.android/tunante-release.jks     PKCS12, RSA 4096, alias "tunante"
~/.android/tunante-release.pass    its password
```

SHA-256 `e9:30:2e:1f:23:f3:68:b4:6b:9e:cd:b7:5a:c0:e7:bd:29:77:79:49:67:d6:d4:59:fb:87:ed:98:e8:7b:f4:54`

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
