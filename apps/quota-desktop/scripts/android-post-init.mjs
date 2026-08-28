import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const manifestUrl = new URL(
  "../src-tauri/gen/android/app/src/main/AndroidManifest.xml",
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

export async function main() {
  const path = fileURLToPath(manifestUrl);
  const source = await readFile(path, "utf8");
  const hardened = hardenAndroidManifest(source);
  if (hardened === source && !source.includes('android:allowBackup="false"')) {
    throw new Error(`AndroidManifest.xml 缺少 <application>：${path}`);
  }
  if (hardened !== source) await writeFile(path, hardened, "utf8");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
