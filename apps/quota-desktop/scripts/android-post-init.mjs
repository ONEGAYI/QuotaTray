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

export function initializeAndroidKeyringInMainActivity(source) {
  const newline = source.includes("\r\n") ? "\r\n" : "\n";
  let initialized = source;
  if (!initialized.includes("import io.crates.keyring.Keyring")) {
    initialized = initialized.replace(
      "import androidx.activity.enableEdgeToEdge",
      `import androidx.activity.enableEdgeToEdge${newline}import io.crates.keyring.Keyring`,
    );
  }
  if (!initialized.includes("Keyring.initializeNdkContext(applicationContext)")) {
    initialized = initialized.replace(
      /(override fun onCreate\(savedInstanceState: Bundle\?\) \{\r?\n)/,
      `$1    Keyring.initializeNdkContext(applicationContext)${newline}`,
    );
  }
  return initialized;
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
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
