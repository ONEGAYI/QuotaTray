@echo off
rem QuotaTray git hooks setup: point core.hooksPath to the in-repo .githooks dir.
rem Idempotent - safe to run repeatedly. Run from the repository root.
rem Unix/macOS equivalent: git config core.hooksPath .githooks
setlocal
git config core.hooksPath .githooks
if errorlevel 1 (
    echo [hooks] setup failed: run from the repository root.
    endlocal & exit /b 1
)
for /f "delims=" %%i in ('git config core.hooksPath') do set "HOOKS_PATH=%%i"
echo [hooks] core.hooksPath = %HOOKS_PATH%
echo [hooks] pre-commit: cargo fmt --check + pnpm lint    (seconds)
echo [hooks] pre-push   : cargo clippy -D warnings + tsc  (minutes, PR gate)
endlocal & exit /b 0
