@echo off
setlocal

set "ROOT=%~dp0.."
set "DIST=%ROOT%\dist\argos-explorer-windows-x64"
set "ARCHIVE=%ROOT%\dist\argos-explorer-windows-x64.zip"
set "SETUP=%ROOT%\dist\argos-explorer-setup.exe"
set "CHECKSUM=%ROOT%\dist\argos-explorer-setup.exe.sha256"
set "SETUP_PDB=%ROOT%\dist\argos-explorer-setup.pdb"

pushd "%ROOT%" || exit /b 1
cargo build --release
if errorlevel 1 (
  popd
  exit /b 1
)

if exist "%DIST%" rmdir /s /q "%DIST%"
if exist "%ARCHIVE%" del /q "%ARCHIVE%"
if exist "%SETUP%" del /q "%SETUP%"
if exist "%CHECKSUM%" del /q "%CHECKSUM%"
if exist "%SETUP_PDB%" del /q "%SETUP_PDB%"
mkdir "%DIST%" || (
  popd
  exit /b 1
)

set "ARGOS_EXPLORER_INSTALLER_BINARY=%ROOT%\target\release\argos-explorer.exe"
set "ARGOS_EXPLORER_INSTALLER_CONFIG=%ROOT%\config.example.toml"
set "ARGOS_EXPLORER_INSTALLER_NOTICES=%ROOT%\THIRD-PARTY-NOTICES.txt"
set "ARGOS_EXPLORER_INSTALLER_INSTRUCTIONS=%ROOT%\INSTALL-WINDOWS.txt"
rustc --edition=2024 -D warnings -C opt-level=z -C debuginfo=0 -C strip=symbols "scripts\installer.rs" -o "%SETUP%"
if errorlevel 1 goto :error
if exist "%SETUP_PDB%" del /q "%SETUP_PDB%"

copy /y "target\release\argos-explorer.exe" "%DIST%\argos-explorer.exe" >nul || goto :error
copy /y "config.example.toml" "%DIST%\config.example.toml" >nul || goto :error
copy /y "THIRD-PARTY-NOTICES.txt" "%DIST%\THIRD-PARTY-NOTICES.txt" >nul || goto :error
copy /y "INSTALL-WINDOWS.txt" "%DIST%\INSTALL-WINDOWS.txt" >nul || goto :error

tar.exe -a -c -f "%ARCHIVE%" -C "%DIST%" argos-explorer.exe config.example.toml THIRD-PARTY-NOTICES.txt INSTALL-WINDOWS.txt
if errorlevel 1 goto :error

set "HASH="
for /f "skip=1 delims=" %%H in ('certutil -hashfile "%SETUP%" SHA256') do if not defined HASH set "HASH=%%H"
if not defined HASH goto :error
<nul set /p "=%HASH: =% *argos-explorer-setup.exe">"%CHECKSUM%"

popd
echo Created %ARCHIVE%
echo Created %SETUP%
echo Created %CHECKSUM%
exit /b 0

:error
popd
exit /b 1
