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
const notificationHelperUrl = new URL(
  "../src-tauri/gen/android/app/src/main/java/com/quotatray/android/NotificationHelper.kt",
  import.meta.url,
);
const backgroundWorkerUrl = new URL(
  "../src-tauri/gen/android/app/src/main/java/com/quotatray/android/BackgroundWorker.kt",
  import.meta.url,
);
const proguardKeepRulesUrl = new URL(
  "../src-tauri/gen/android/app/proguard-quotatray.pro",
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
import android.util.Log

/**
 * APK 安装引导桥：Rust 侧（src-tauri/src/apk_install.rs）经 JNI 调用，
 * 以 ACTION_VIEW 把 SAF 保存的 APK 交给系统安装器；不走应用自安装
 * 通道（Play 审核高危项），由系统安装器经用户确认接管，无分发红线。
 *
 * 本类仅被 Rust 反射加载（loadClass），无任何 Java/Kotlin 静态引用——
 * release 构建的 R8 会将其视作无引用代码收缩改名，keep 规则见
 * proguard-quotatray.pro（同样由本脚本注入）。两个入口都必须经主线程
 * Handler 派发 startActivity（Rust 命令线程发起，非 UI 线程）。
 *
 * 「安装未知应用」闸口（Android 8+）：其许可状态**程序化不可知**——
 * PackageManager 与 AppOpsManager 的查询 API 均要求调用方先声明自安装
 * 权限（本项目永不声明，调用即 SecurityException，API 36 实证
 * 2026-08-29）。因此不做预判：openApk 直接发安装请求——Android 7 无需
 * 闸口直接弹确认页，8~15 上授权放行时一步到位；未声明权限时新版系统
 * （API 36 实证：AppOps allow 亦被弹回，授权页开关置灰）一律 toast 弹
 * 回，前端提示行以「文件管理器打开已保存 APK」为主出路，并提供
 * openInstallConsent 的授权页入口作为旧版系统的次出路（公开 Settings
 * action，同样不触碰权限声明；该 action 为 API 26 引入，更低版本返回
 * false 由前端降级）。
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
            } catch (e: ActivityNotFoundException) {
                // resolveActivity 与启动之间的竞态兜底：安装器被禁用则放弃；
                // 留应用侧日志便于「点了没反应」类报告取证（不依赖系统 logcat）
                Log.w("QuotaTrayApkInstall", "系统安装器竞态缺失：\${e.message}")
            }
        }
        return true
    }

    /**
     * 打开本应用的「允许安装未知应用」系统授权页（用户开关，非权限声明）。
     * API 26 以下系统无此 Settings action，返回 false 交由调用方降级。
     */
    @JvmStatic
    fun openInstallConsent(context: Context): Boolean {
        if (android.os.Build.VERSION.SDK_INT < 26) return false
        val consent = Intent(
            android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
            Uri.parse("package:\${context.packageName}"),
        ).apply { addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
        return Handler(Looper.getMainLooper()).post {
            try {
                context.startActivity(consent)
            } catch (e: ActivityNotFoundException) {
                Log.w("QuotaTrayApkInstall", "授权设置页缺失：\${e.message}")
            }
        }
    }
}
`;
}

export function androidNotificationHelperSource() {
  return `package com.quotatray.android

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.util.Log

/**
 * 系统通知设置页跳转桥：Rust 侧（src-tauri/src/notification_android.rs）
 * 经 JNI 调用，打开本应用的「应用通知设置」页——Android 13+ 用户拒绝过
 * 通知运行时权限后系统对话框不再弹出，跳系统设置是唯一授权出路
 * （用户开关授权，不做任何通知权限绕过）。
 *
 * 本类仅被 Rust 反射加载（loadClass），keep 规则见
 * proguard-quotatray.pro。startActivity 必须经主线程 Handler 派发
 * （Rust 命令线程发起，非 UI 线程）。
 *
 * ACTION_APP_NOTIFICATION_SETTINGS 为 API 26 引入；本应用 minSdk 24，
 * 更低版本返回 false 由前端降级为纯文案引导。
 */
object NotificationHelper {
    @JvmStatic
    fun openNotificationSettings(context: Context): Boolean {
        if (android.os.Build.VERSION.SDK_INT < 26) return false
        val settings = Intent(
            android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS,
        ).apply {
            putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, context.packageName)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        return Handler(Looper.getMainLooper()).post {
            try {
                context.startActivity(settings)
            } catch (e: ActivityNotFoundException) {
                Log.w("QuotaTrayNotification", "通知设置页缺失：\${e.message}")
            }
        }
    }
}
`;
}

/**
 * R8 keep 规则：ApkInstallHelper（APK 安装链）、NotificationHelper
 * （系统通知设置页跳转）仅被 Rust 侧反射加载（无 Java 引用）；
 * BackgroundWorker（WorkManager 反射实例化 + external fun 符号绑定）、
 * Native（external fun 符号绑定）、BackgroundScheduler（Rust 反射调用）
 * 同样无常规 Java 引用路径。release 构建（isMinifyEnabled=true）会将其
 * 收缩改名，导致 Rust loadClass 抛 ClassNotFoundException / JNI 符号
 * UnsatisfiedLinkError、对应链路恒失败。build.gradle.kts 的
 * proguardFiles 已以 fileTree 收编 app 目录全部 .pro，落文件即生效。
 */
export function proguardKeepRulesSource() {
  return `-keep class com.quotatray.android.ApkInstallHelper { *; }
-keep class com.quotatray.android.NotificationHelper { *; }
-keep class com.quotatray.android.Native { *; }
-keep class com.quotatray.android.BackgroundWorker { *; }
-keep class com.quotatray.android.BackgroundScheduler { *; }
`;
}

/**
 * 后台刷新三件套（C 项）：Native（external fun 桥）、BackgroundWorker
 * （WorkManager 周期任务）、BackgroundScheduler（Rust 反射调用的调度
 * 入口）。文件由脚本整体生成（writeIfChanged 幂等），行为由契约测试
 * 锁定；Rust 侧对应实现在 src-tauri/src/background.rs。
 */
export function androidBackgroundWorkerSource() {
  return `package com.quotatray.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.os.Build
import android.util.Log
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import androidx.core.app.NotificationManagerCompat
import io.crates.keyring.Keyring
import java.util.concurrent.TimeUnit
import org.json.JSONObject

/**
 * JNI 桥：Rust 侧（src-tauri/src/background.rs）导出的后台刷新入口。
 * 独立 object 而非 companion——companion 的 external fun 符号名含 \$ 转义
 * 陷阱。loadLibrary 与 tauri 运行时加载同一 so，重复调用幂等。
 */
object Native {
    init {
        System.loadLibrary("quota_desktop_lib")
    }

    /** 返回 JSON：{channel:{id,name}, notifications:[{title,body}]}（Rust 组装）。 */
    external fun backgroundRefresh(dataDir: String): String
}

/**
 * 后台刷新 Worker（C 项）：WorkManager 周期调度（最小 15 分钟，系统
 * 硬限），经 JNI 调 Rust 编排核完成「查询 → 写历史库 → 低余额边沿
 * 判定」，通知由 Kotlin 直发（渠道元数据随返回 JSON 携带，Rust 单一
 * 数据源）。查询失败 Rust 侧已静默（桌面调度器同口径），Worker 永不
 * retry——下个周期自然重试。
 *
 * 冷启动（WorkManager 拉起已死进程）：MainActivity 与 tauri 运行时均
 * 不在，Keyring.initializeNdkContext 补调（幂等）保证 Rust 侧 Keystore
 * 凭据链可用——这是 doWork 的第一步。
 */
class BackgroundWorker(appContext: Context, params: WorkerParameters) :
    Worker(appContext, params) {

    override fun doWork(): Result {
        return try {
            // 冷启动补调（幂等）：Rust 侧 vault 经 ndk-context 取 Context
            Keyring.initializeNdkContext(applicationContext)
            // dataDir（Context.getDataDir，API 24+）：与 tauri 前台的
            // app_data_dir 同源（PathPlugin 经 activity.dataDir 解析）。
            // 不得用 filesDir（其下再拼 files/ 与前台目录错位，Worker
            // 读不到 settings.json，后台刷新整体失效——审查 M1）
            val dataDir = applicationContext.dataDir.absolutePath
            val json = try {
                Native.backgroundRefresh(dataDir)
            } catch (e: Throwable) {
                Log.w(TAG, "JNI 调用异常：\${e.message}")
                null
            }
            if (json == null) {
                // Rust 侧 new_string 兜底仍失败（OOM 级）才会返回 null；
                // 无通知可发，静默成功等下个周期
                Log.w(TAG, "backgroundRefresh 返回 null")
                return Result.success()
            }
            Log.i(TAG, "刷新完成：\$json")
            dispatchNotifications(json)
            Result.success()
        } catch (e: Throwable) {
            // 解析/通知异常兜底：静默成功（不 retry），下个周期重试
            Log.w(TAG, "后台刷新失败：\${e.message}")
            Result.success()
        }
    }

    /** 按返回 JSON 直发系统通知：先幂等建渠道（API 26+），未授权整体
     *  跳过（areNotificationsEnabled 覆盖 Android 13+ 运行时权限与更早
     *  系统级开关）。通知 id 基址 + 序号，多条不互相覆盖。 */
    private fun dispatchNotifications(json: String) {
        val result = JSONObject(json)
        val channel = result.getJSONObject("channel")
        val channelId = channel.getString("id")
        val channelName = channel.optString("name", "").ifEmpty { "QuotaTray" }
        val manager =
            applicationContext.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= 26) {
            manager.createNotificationChannel(
                NotificationChannel(channelId, channelName, NotificationManager.IMPORTANCE_DEFAULT),
            )
        }
        val notifications = result.optJSONArray("notifications") ?: return
        val compat = NotificationManagerCompat.from(applicationContext)
        if (!compat.areNotificationsEnabled()) return
        for (i in 0 until notifications.length()) {
            val item = notifications.getJSONObject(i)
            val builder = if (Build.VERSION.SDK_INT >= 26) {
                Notification.Builder(applicationContext, channelId)
            } else {
                @Suppress("DEPRECATION")
                Notification.Builder(applicationContext)
            }
            compat.notify(
                NOTIFY_ID_BASE + i,
                builder
                    .setSmallIcon(android.R.drawable.stat_notify_chat)
                    .setContentTitle(item.getString("title"))
                    .setContentText(item.getString("body"))
                    .setAutoCancel(true)
                    .build(),
            )
        }
    }

    companion object {
        private const val TAG = "QuotaTrayBackground"
        private const val NOTIFY_ID_BASE = 20_001
    }
}

/**
 * 调度入口：Rust 侧（persist_settings 落盘后与应用启动 setup）经 JNI
 * 反射调用 schedule(Context, Boolean, Long)。UPDATE 策略——spec 变化
 * （间隔变更）即时生效且保留周期对齐，spec 不变则 no-op；开关关则
 * cancelUniqueWork。间隔下限 15 分钟双保险（Rust sanitize 已收口，
 * 此处再 coerce 防手动改 settings.json 的越界值）。
 */
object BackgroundScheduler {
    private const val UNIQUE_WORK = "quotatray-background-refresh"

    @JvmStatic
    fun schedule(context: Context, enabled: Boolean, intervalMinutes: Long): Boolean {
        val manager = WorkManager.getInstance(context)
        if (!enabled) {
            manager.cancelUniqueWork(UNIQUE_WORK)
            return true
        }
        val interval = intervalMinutes.coerceAtLeast(15)
        val request = PeriodicWorkRequestBuilder<BackgroundWorker>(
            interval,
            TimeUnit.MINUTES,
        )
            .setConstraints(
                Constraints.Builder()
                    .setRequiredNetworkType(NetworkType.CONNECTED)
                    .build(),
            )
            .build()
        manager.enqueueUniquePeriodicWork(UNIQUE_WORK, ExistingPeriodicWorkPolicy.UPDATE, request)
        return true
    }
}
`;
}

/**
 * build.gradle.kts 注入 androidx.work 依赖（WorkManager）。锚点漂移
 * （上游模板重构 dependencies 块）即抛错，与签名注入同防线；幂等
 * 标记用依赖坐标本身（上游将来原生携带该依赖时不重复注入）。
 */
export function injectAndroidWorkManagerDependency(source) {
  const marker = "androidx.work:work-runtime-ktx";
  if (source.includes(marker)) return source;
  const newline = source.includes("\r\n") ? "\r\n" : "\n";
  const anchor = source.match(/^dependencies \{$/m);
  if (!anchor) {
    throw new Error("build.gradle.kts 缺少 dependencies 锚点");
  }
  return source.replace(
    anchor[0],
    () => `dependencies {${newline}    implementation("${marker}:2.9.1")`,
  );
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
  await writeIfChanged(
    fileURLToPath(notificationHelperUrl),
    androidNotificationHelperSource(),
  );
  await writeIfChanged(
    fileURLToPath(backgroundWorkerUrl),
    androidBackgroundWorkerSource(),
  );
  await writeIfChanged(
    fileURLToPath(proguardKeepRulesUrl),
    proguardKeepRulesSource(),
  );

  const buildGradlePath = fileURLToPath(buildGradleUrl);
  const buildGradle = await readFile(buildGradlePath, "utf8");
  // keep 规则文件依赖上游模板的 fileTree 收编（app 目录全部 .pro 进
  // proguardFiles）；上游若改为显式文件名列表，keep 会静默失效且契约
  // 测试全绿——锚点漂移即抛错，与签名注入同防线（R2 复验 2026-08-29）
  if (!buildGradle.includes('fileTree(".") { include("**/*.pro") }')) {
    throw new Error(
      `build.gradle.kts 缺少 fileTree("**/*.pro") 收编锚点，keep 规则将失效：${buildGradlePath}`,
    );
  }
  const workGradle = injectAndroidWorkManagerDependency(buildGradle);
  // 后验与注入幂等标记同源（坐标前缀，不含版本）——上游将来原生携带
  // 其他版本时注入按设计放行原文，硬编码版本的断言会误伤 fail-fast
  if (!workGradle.includes("androidx.work:work-runtime-ktx")) {
    throw new Error(`build.gradle.kts 未能注入 WorkManager 依赖：${buildGradlePath}`);
  }
  const injectedGradle = injectAndroidReleaseSigning(workGradle);
  if (!injectedGradle.includes('signingConfigs.findByName("release")')) {
    throw new Error(`build.gradle.kts 未能注入 release 签名配置：${buildGradlePath}`);
  }
  await writeIfChanged(buildGradlePath, injectedGradle);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
