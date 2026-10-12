@echo off
rem UnRAR — release build (Rust / egui)
cargo build --release || exit /b 1
if not exist dist mkdir dist
copy /y target\release\UnRAR.exe dist\UnRAR.exe
echo Built dist\UnRAR.exe
