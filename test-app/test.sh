#!/usr/bin/env bash

cd test-app
dx serve --platform ios > ../dx_serve.log 2>&1 &
DX_PID=$!
cd ..

# Clean up background server process when step finishes
trap "kill -9 $DX_PID 2>/dev/null || true" EXIT
