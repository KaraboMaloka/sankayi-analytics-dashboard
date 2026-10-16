@echo off
REM Nightly Quicket sales sync for Sankayi Intelligence.
REM Runs the release `ingest sales` binary from the backend dir (so .env loads),
REM appending output to ingest.log. Registered as a Windows scheduled task.
cd /d "C:\gitrepos\sankayi-analytics-dashboard\backend"
echo.>> ingest.log
echo [%date% %time%] --- nightly sales sync starting --->> ingest.log
"C:\gitrepos\sankayi-analytics-dashboard\backend\target\release\ingest.exe" sales >> ingest.log 2>&1
echo [%date% %time%] finished with exit code %errorlevel%>> ingest.log
