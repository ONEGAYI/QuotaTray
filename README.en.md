# QuotaTray

English | **[中文](README.md)**

A tray-resident multi-platform AI account balance monitor: built-in queries for official platforms, a declarative template system for everything else — with credentials always encrypted, never stored in plaintext.

![OS](https://img.shields.io/badge/OS-Windows%20%7C%20Android%20Preview-blue) ![License](https://img.shields.io/badge/License-MIT-green)

## Why QuotaTray

If you hold accounts across multiple AI platforms — DeepSeek, Kimi, OpenRouter and so on — balances and quotas are scattered across their web consoles, each behind its own login.

QuotaTray compresses that into a single glance: it lives in the system tray, the icon *is* your balance status, and hovering or opening the tray menu shows every platform's balance and usage percentage.

The key difference from other balance tools is **credential security**:

- API keys are stored as AES-256-GCM ciphertext in the regular config; the machine master key is held by the OS credential vault and is **never exported**.
- A copied regular config file cannot be decrypted off this machine. An explicitly generated cross-machine transfer package carries a one-time transfer key and must be protected like plaintext credentials.
- The project only *reads* balances and never writes into any CLI tool's config files — that is precisely what allows credentials to stay encrypted end to end.

## Feature Overview

**Tray & UI (desktop)**

- Tray ring icon: layered arcs per entry, color shifts when balance drops below threshold
- Tray menu lists balance / usage percentage and last-updated time per entry, plus two lines for off-peak pricing
- Hover detail panel: balance-first summary with quick switching of the ring data-source account and pricing model
- Main window with card list: add/edit entries, template editor (with validation and live test), structured off-peak pricing editor
- Settings → Data management: cross-machine configuration export/import (with sensitive-file confirmation) and one-click clear of all user data (5-second countdown double confirmation)
- Theme tri-state (light/dark/system), bilingual UI tri-state, custom title bar
- Keep-last-good: on query failure the last good result keeps showing within its time window; after restart the snapshot renders first — no blank window
- Windows ARM64 (Preview): standalone and portable zips with native ARM64 GUI and CLI binaries
- Portable build (Windows x64 / ARM64 Preview): all data travels in the `Data/` folder, first-run security confirmation, delete the folder to uninstall

**Android Preview**

- Reuses the same Rust core, configuration format, and provider implementations; the master key is protected by Android Keystore
- Touch-first bottom navigation, top app bar, full-screen editors, and click-driven disclosure states instead of hover-only interactions
- The first preview only promises foreground refresh; there is no tray, autostart, background worker, desktop updater, or bundled CLI
- Releases carry a signed ARM64 APK built automatically by the release-tag CI; Android remains Preview until full physical-device acceptance

See the Chinese [Android Preview implementation note](docs/Android端预览版说明.md) for security boundaries and the acceptance checklist.

**Command line (quota-cli, sharing the same core as the GUI)**

```text
quota natives                  # list built-in platforms
quota add                      # interactive wizard (masked key input)
quota query                    # query all enabled entries in parallel
quota query --watch            # polling mode
quota pricing show <id>        # effective off-peak pricing & current period verdict
quota template test --json     # template validation + live query
quota config export <path>     # export a complete transfer package (asks for confirmation)
quota config import <path>     # replace all and re-encrypt with this machine's key
quota clear --yes              # wipe all user data (explicit --yes required non-interactively)
quota update --check           # check for new releases
```

- Every command supports `--json` output for scripting
- Three-way exit codes: `0` all success / `1` deterministic failures / `2` transient only
- Bilingual tri-state messages (`--lang zh|en|system`)

**Off-peak pricing**

- Peak/off-peak windows by weekday + time of day, three prices per tier: cache-hit / cache-miss / output (per MTokens)
- DeepSeek's official pricing ships built-in; entries can override field by field (empty fields fall back to preset)
- Custom model library: add models and prices per platform; entry pricing can opt in
- Both tray and CLI show the current period verdict and the next flip time

**Update checks**

- Periodic GitHub release checks (frequency and time-of-day configurable; tray menu shows a new-version line)
- Manual check and installer download (to the system download folder; no auto-install)

## Built-in Platforms

| Platform | Site | Notes |
|---|---|---|
| DeepSeek | — | single site, dual currency from balance API |
| SiliconFlow | CN / Global | CNY / USD |
| OpenRouter | — | remaining = credits − usage |
| Kimi Open Platform | CN / Global | balance split into voucher / cash |
| Kimi Code | kimi.com/code / kimi.ai/code | 5-hour + weekly quota windows with RFC3339 reset times |
| Zhipu / Z.ai General API | CN / Global | pay-as-you-go balance, Bearer API key |
| Zhipu / Z.ai Coding Plan | CN / Global | plan usage (multi-window), raw key |
| StepFun | — | top-level balance, CNY |
| Novita AI | — | availableBalance ÷ 10000 = USD |
| MiniMax Coding Plan | CN / Global | 5h + weekly remaining percent, normalized to used |
| Claude subscription | — | Pro/Max multi-window usage; credentials read from the local Claude Code login |
| Codex (ChatGPT subscription) | — | Plus/Pro dual-window usage; credentials read from the local Codex CLI login |
| Gemini Code Assist | — | per-model-group remaining quota; credentials read from the local Gemini CLI login |
| Grok subscription | — | SuperGrok credits usage; credentials read from the local Grok CLI login |

Subscription platforms (Claude/Codex/Gemini/Grok) need **no API key**: credentials are read-only from the locally signed-in official CLI's login file at query time—never written or exported. Kimi Code uses the usage endpoint adopted by MoonshotAI's official client; the Zhipu / Z.ai and the two subscription query endpoints plus credential file formats are not in public API references (stable contracts in wide community use); the others use official public APIs. Automated tests are fully mocked. **Platforms not listed can be added via declarative templates** (next section).

## Custom Queries: Declarative Templates

Most balance APIs are "one GET + auth header + field mapping ± arithmetic". A JSON description is all it takes — no code:

```json
{
  "request": {
    "url": "{{baseUrl}}/v1/user/info",
    "headers": { "Authorization": "Bearer {{apiKey}}" }
  },
  "extract": {
    "remaining": "$.data.totalBalance",
    "unit": { "const": "CNY" }
  },
  "transforms": [
    { "op": "multiply", "field": "remaining", "by": 0.01 }
  ],
  "windows": []
}
```

- `extract` takes values via a JSONPath subset (`$.a.b[0]`) or constants
- `transforms` provides restricted arithmetic (multiply/divide/add/sub/round); no eval at runtime
- `windows` unfolds homogeneous quota arrays; heterogeneous responses such as Kimi Code are handled by a built-in provider
- Templates are statically validated on save; URLs must be HTTPS and same-origin with `{{baseUrl}}` (loopback excepted)

Runnable examples live in [examples/templates/](examples/templates/): single-object extraction (string numbers), dual-site `{{baseUrl}}`, total/usage display, and multi-window unfolding — each verifiable with `quota template test`.

## Custom Queries: JS Scripts

Complex platforms beyond the template DSL's restricted arithmetic (cross-field math, loop aggregation, response reshaping) fall back to JS scripts — a `{request, extractor}` two-phase protocol inside a QuickJS sandbox, with HTTP executed by the host:

```js
function request() {
  // Return a request descriptor; {{apiKey}} / {{baseUrl}} are injection placeholders
  // (string-level substitution — scripts can be shared safely)
  return { url: "{{baseUrl}}/v1/quota", headers: { "Authorization": "Bearer {{apiKey}}" } };
}
function extract(resp) {
  // resp = the parsed response JSON; return a single object or a multi-window array (UsageData shape)
  return [{ plan_name: "week", used: resp.week.used, unit: "%", reset_at: Date.parse(resp.week.reset) }];
}
```

Sandbox limits: 16 MiB memory, a 5-second CPU cap per execution, no network/filesystem; URL safety and redaction rules match the templates (echoed secrets in error messages are masked). Runnable examples live in [examples/scripts/](examples/scripts/), verifiable with `quota script test`.

## Install

### Windows x64

Download the NSIS installer (`*-setup.exe`) from [Releases](https://github.com/ONEGAYI/QuotaTray/releases).

### Windows on ARM

- **ARM64 (Preview)**: download `*-arm64-preview.zip`, extract it, and run `QuotaTray.exe`. This archive has no installer and continues to use the installed-mode data directory and system credential store.
- **ARM64 (Preview), portable**: download `*-arm64-preview-portable.zip`; its data and portable master key live in the adjacent `Data/` folder.

> 🧪 **ARM64 Preview**: The ARM64 build has passed cross-compilation and artifact architecture checks, but has not completed full runtime validation on a physical Windows on ARM device. It is provided for preview and feedback only and must not be treated as stable support.

### Portable (Windows x64 / ARM64 Preview)

For Windows x64, download `*-x64-portable.zip`; for ARM64 Preview, download
`*-arm64-preview-portable.zip`. Extract to any writable folder and run — all data stays
in the adjacent `Data/` folder; deleting the folder uninstalls. First run shows a
security confirmation. The zip contains the GUI (`QuotaTray.exe`) and the CLI
(`quota.exe`), sharing the same `Data/` folder.

> ⚠️ **Portable security notice**: the portable build stores the master key that decrypts your credentials in `Data/portable.key`. Although credentials remain AES-GCM encrypted in the configuration, the key and the ciphertext live in the same portable directory, so the entire `Data/` folder must be treated as plaintext credentials. Do not upload it to cloud drives, commit it to version control, or share it with others; if the medium is lost or the folder leaks, rotate every API key it contained immediately.

### Android (Preview)

Download the signed APK (`QuotaTray_<version>_android-arm64.apk`, ARM64 devices) from
[Releases](https://github.com/ONEGAYI/QuotaTray/releases) and install it. The APK is
signed with a long-term key by the release-tag CI; same-signature versions upgrade
in place without uninstalling (verified on emulator; physical-device acceptance in
progress).

> 🧪 **Android Preview**: the Android build has passed emulator smoke acceptance but has
> not completed full acceptance on physical devices. Treat the asset as preview-only,
> not stable support.

### Build from source

Requires: Rust stable, Node.js, pnpm.

```bash
# Desktop installer (NSIS output under apps/quota-desktop)
cd apps/quota-desktop
pnpm install
pnpm tauri build

# From the repository root: x64 setup + portable
cd ../..
.\package

# WoA standalone + portable Preview zips (requires ARM64 MSVC + clang)
.\package -Arch arm64

# CLI only
cargo build -p quota-cli --release
```

### Clean the development workspace

Run `clean` from the repository root on Windows. Omitting the level opens an
interactive selector:

```powershell
.\clean 1              # Light: incremental/Vite caches and generated output
.\clean 2              # Standard: also remove target/debug; keep release output
.\clean 3              # Deep: full target + node_modules + generated output
.\clean 3 -WhatIf      # Preview targets without deleting anything
```

The cleaner only touches a fixed allowlist of paths inside the repository. It
never removes source files, `.git`, development keys, `.zcode`, or uncommitted
files. After Level 3, run `pnpm install` again under `apps/quota-desktop`; Rust
dependencies will also be rebuilt on the next build.

## Security Design

The key hierarchy:

```
OS credential vault (Windows Credential Manager)
  └─ Master key: 32 random bytes, generated on first run, never written to disk
        │ AES-256-GCM
        ▼
Credential fields in ~/.quotatray/config.json (v1:<base64>, versioned)
```

- The master key is unique per machine and has zero correlation with the source code
- The ciphertext format is versioned for smooth future algorithm migration
- The GCM authentication tag guarantees integrity — tampering fails decryption
- Credentials in logs and error messages are always masked (`sk-****<last4>`)
- The web frontend never receives plaintext credentials: queries happen in the local backend and the UI only shows results; editing credentials goes through a write-only channel with no echo

Global sensitivity comparison across the three data forms:

| Form | Master key location | Sensitivity | Handling |
| --- | --- | --- | --- |
| Installed (default) | OS credential vault, never on disk | Config file is undecipherable off this machine | — |
| Portable | `Data/portable.key`, next to the ciphertext | The whole `Data/` folder equals plaintext credentials | Never upload to cloud drives / commit / share; on leak, rotate every API key |
| Transfer package `.qtray-export` | Fresh one-time key generated per export, carried in the package | Equals plaintext credentials | Delete after migration |

Cross-machine migration uses the private `.qtray-export` binary container. On every export,
core generates a fresh 32-byte one-time transfer key, rewraps the source credentials, and
authenticates the encrypted configuration as a whole. Import rewraps those credentials under
the destination machine's master key.
Use `quota config export/import` in the CLI, or open **Settings → Data management** in the
desktop app. Both require explicit confirmation by default; CLI automation can pass `--yes`.

Clearing all data (**Settings → Data management** in the desktop app, or `quota clear`)
removes every provider entry, encrypted credential, pricing configuration and query
history, while keeping the master key and app preferences — it is an in-place cleanup
step before transferring or troubleshooting, not a "secure erase" promise (filesystem
level remnants are out of scope).

Known boundaries: reading the OS credential vault from another process of the same user is out of scope (same assurance level as browser-saved passwords); memory attacks and local malware are beyond a desktop tool's threat model.

## Roadmap

- [x] QuickJS sandbox script queries (`{request, extractor}` protocol; memory/CPU limits; no network, no filesystem)
- [ ] More built-in platforms
- [ ] Automatic update installation

## Acknowledgements

The unified result model and the dual-track error classification borrow from the practice of [cc-switch](https://github.com/farion1231/cc-switch) (MIT licensed). Thanks for open-sourcing it.

## License

[MIT](LICENSE) © 2026 ONEGAYI
