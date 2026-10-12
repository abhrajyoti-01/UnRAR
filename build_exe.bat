@echo off
rem Build the UnRAR executable (requires: python -m pip install pyinstaller)
rem Produces dist\UnRAR.exe (GUI). The CLI remains available from source:
rem   python unrar.py l|t|x|a|i <args>

python -m PyInstaller --noconfirm --clean --onefile --windowed ^
  --name UnRAR --version-file version.txt unrar_gui.py
if errorlevel 1 goto :fail

echo.
echo Built: dist\UnRAR.exe  (GUI)
goto :eof

:fail
echo Build failed.
exit /b 1
