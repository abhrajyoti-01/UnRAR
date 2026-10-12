@echo off
rem Build UnRAR distributable executables (requires: python -m pip install pyinstaller)
rem Produces dist\UnRAR.exe (GUI) and dist\unrar.exe (console CLI) from the same code.

python -m PyInstaller --noconfirm --clean --onefile --windowed ^
  --name UnRAR --version-file version.txt unrar_gui.py
if errorlevel 1 goto :fail

python -m PyInstaller --noconfirm --clean --onefile --console ^
  --name unrar-cli --version-file version.txt unrar.py
if errorlevel 1 goto :fail

echo.
echo Built: dist\UnRAR.exe  (GUI)   dist\unrar-cli.exe  (CLI)
goto :eof

:fail
echo Build failed.
exit /b 1
