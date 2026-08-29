import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const manifestUrl = new URL(
  "../src-tauri/gen/android/app/src/main/AndroidManifest.xml",
  import.meta.url,
);
const mainActivityUrl = new URL(
  "../src-tauri/gen/android/app/src/main/java/com/quotatray/android/MainActivity.kt",
  import.meta.url,
);
const keyringBridgeUrl = new URL(
  "../src-tauri/gen/android/app/src/main/java/io/crates/keyring/Keyring.kt",
  import.meta.url,
);
const apkInstallHelperUrl = new URL(
  "../src-tauri/gen/android/app/src/main/java/com/quotatray/android/ApkInstallHelper.kt",
  import.meta.url,
);
const buildGradleUrl = new URL(
  "../src-tauri/gen/android/app/build.gradle.kts",
  import.meta.url,
);

export function hardenAndroidManifest(source) {
  if (!source.includes("<application")) return source;
  let hardened = source.match(/android:allowBackup="[^"]*"/)
    ? source.replace(/android:allowBackup="[^"]*"/, 'android:allowBackup="false"')
    : source.replace("<application", '<application android:allowBackup="false"');
  hardened = hardened.match(/android:fullBackupContent="[^"]*"/)
    ? hardened.replace(
        /android:fullBackupContent="[^"]*"/,
        'android:fullBackupContent="false"',
      )
    : hardened.replace(
        "<application",
        '<application android:fullBackupContent="false"',
      );
  return hardened;
}

export function androidKeyringBridgeSource() {
  return `package io.crates.keyring

import android.content.Context

class Keyring {
  companion object {
    init {
      System.loadLibrary("quota_desktop_lib")
    }

    external fun initializeNdkContext(context: Context)
  }
}
`;
}

export function androidApkInstallHelperSource() {
  return `package com.quotatray.android

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper

/**
 * APK 安装引导桥：Rust 侧（src-tauri/src/apk_install.rs）经 JNI 调用，
 * 以 ACTION_VIEW 把 SAF 保存的 APK 交给系统安装器；不走应用自安装
 * 通道（Play 审核高危项），由系统安装器经用户确认接管，无分发红线。
 *
 * 「安装未知应用」闸口（Android 8+）：其许可状态**程序化不可知**——
 * PackageManager 与 AppOpsManager 的查询 API 均要求调用方先声明自安装
 * 权限（本项目永不声明，调用即 SecurityException，API 36 实证
 * 2026-08-29）。因此不做预判：openApk 直接发安装请求——Android 7 无需
 * 闸口直接弹确认页，8~15 上授权放行时一步到位；未声明权限时新版系统
 * （API 36 实证：AppOps allow 亦被弹回，授权页开关置灰）一律 toast 弹
 * 回，前端提示行以「文件管理器打开已保存 APK」为主出路，并提供
 * openInstallConsent 的授权页入口作为旧版系统的次出路（公开 Settings
 * action，同样不触碰权限声明）。
 */
object ApkInstallHelper {
    @JvmStatic
    fun openApk(context: Context, uriString: String): Boolean {
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(Uri.parse(uriString), "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        if (context.packageManager.resolveActivity(intent, 0) == null) return false
        Handler(Looper.getMainLooper()).post {
            try {
                context.startActivity(intent)
            } catch (_: ActivityNotFoundException) {
                // resolveActivity 与启动之间的竞态兜底：安装器被禁用则放弃
            }
        }
        return true
    }

    /** 打开本应用的「允许安装未知应用」系统授权页（用户开关，非权限声明）。 */
    @JvmStatic
    fun openInstallConsent(context: Context) {
        val consent = Intent(
            android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
            Uri.parse("package:\${context.packageName}"),
        ).apply { addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
        context.startActivity(consent)
    }
}
`;
}

export function initializeAndroidKeyringInMainActivity(source) {
  const newline = source.includes("\r\n") ? "\r\n" : "\n";
  let initialized = source;
  if (!initialized.includes("import io.crates.keyring.Keyring")) {
    const importAnchor = "import androidx.activity.enableEdgeToEdge";
    if (!initialized.includes(importAnchor)) {
      throw new Error("MainActivity.kt 缺少 enableEdgeToEdge import 锚点");
    }
    initialized = initialized.replace(
      importAnchor,
      `${importAnchor}${newline}import io.crates.keyring.Keyring`,
    );
  }
  if (!initialized.includes("Keyring.initializeNdkContext(applicationContext)")) {
    initialized = initialized.replace(
      /(override fun onCreate\(savedInstanceState: Bundle\?\) \{\r?\n)/,
      `$1    Keyring.initializeNdkContext(applicationContext)${newline}`,
    );
  }
  if (!initialized.includes("import io.crates.keyring.Keyring")) {
    throw new Error("MainActivity.kt 未能注入 Android Keyring import");
  }
  return initialized;
}

export function injectAndroidReleaseSigning(source) {
  // 幂等标记用注入块自有特征串，避免上游模板将来自带 signingConfigs 时被误判已注入
  if (source.includes('rootProject.file("keystore.properties")')) return source;
  const newline = source.includes("\r\n") ? "\r\n" : "\n";
  const buildTypesLine = source.match(/^[ \t]*buildTypes \{[ \t]*$/m);
  if (!buildTypesLine) {
    throw new Error("build.gradle.kts 缺少 buildTypes 锚点");
  }
  const releaseLine = source.match(/^[ \t]*getByName\("release"\) \{[ \t]*$/m);
  if (!releaseLine) {
    throw new Error('build.gradle.kts 缺少 getByName("release") 锚点');
  }
  // 无 keystore.properties 时 signingConfigs 为空、release 不挂签名（与 Tauri 模板默认一致）
  const signingConfigsBlock = [
    "    signingConfigs {",
    '        val keystorePropertiesFile = rootProject.file("keystore.properties")',
    "        val keystoreProperties = Properties()",
    "        if (keystorePropertiesFile.exists()) {",
    "            keystorePropertiesFile.reader(Charsets.UTF_8).use { keystoreProperties.load(it) }",
    '            create("release") {',
    '                keyAlias = keystoreProperties["keyAlias"] as String',
    '                keyPassword = keystoreProperties["keyPassword"] as String',
    '                storeFile = file(keystoreProperties["storeFile"] as String)',
    '                storePassword = keystoreProperties["storePassword"] as String',
    "            }",
    "        }",
    "    }",
  ].join(newline);
  const signingHook = '            signingConfigs.findByName("release")?.let { signingConfig = it }';
  // 函数 replacement 避免 match 文本/注入内容中的 $ 序列被 String.replace 展开
  const injected = source
    .replace(buildTypesLine[0], () => `${signingConfigsBlock}${newline}${buildTypesLine[0]}`)
    .replace(releaseLine[0], () => `${releaseLine[0]}${newline}${signingHook}`);
  return injected;
}

async function writeIfChanged(path, contents) {
  let current;
  try {
    current = await readFile(path, "utf8");
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  if (current !== contents) {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, contents, "utf8");
  }
}

export async function main() {
  const manifestPath = fileURLToPath(manifestUrl);
  const source = await readFile(manifestPath, "utf8");
  const hardened = hardenAndroidManifest(source);
  if (hardened === source && !source.includes('android:allowBackup="false"')) {
    throw new Error(`AndroidManifest.xml 缺少 <application>：${manifestPath}`);
  }
  await writeIfChanged(manifestPath, hardened);

  const mainActivityPath = fileURLToPath(mainActivityUrl);
  const mainActivity = await readFile(mainActivityPath, "utf8");
  const initialized = initializeAndroidKeyringInMainActivity(mainActivity);
  if (!initialized.includes("Keyring.initializeNdkContext(applicationContext)")) {
    throw new Error(`MainActivity.kt 缺少 onCreate：${mainActivityPath}`);
  }
  await writeIfChanged(mainActivityPath, initialized);
  await writeIfChanged(
    fileURLToPath(keyringBridgeUrl),
    androidKeyringBridgeSource(),
  );
  await writeIfChanged(
    fileURLToPath(apkInstallHelperUrl),
    androidApkInstallHelperSource(),
  );

  const buildGradlePath = fileURLToPath(buildGradleUrl);
  const buildGradle = await readFile(buildGradlePath, "utf8");
  const injectedGradle = injectAndroidReleaseSigning(buildGradle);
  if (!injectedGradle.includes('signingConfigs.findByName("release")')) {
    throw new Error(`build.gradle.kts 未能注入 release 签名配置：${buildGradlePath}`);
  }
  await writeIfChanged(buildGradlePath, injectedGradle);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
