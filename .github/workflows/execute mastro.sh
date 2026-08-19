#!/usr/bin/env bash

# 1. Wait until the Dioxus backend is serving HTTP requests
echo "Waiting for Dioxus server to boot..."
until curl -s http://localhost:8080/ > /dev/null; do
  sleep 2
done

echo "Dioxus app is up! Running Maestro smoke test..."
sleep 2
# 2. Trigger Maestro directly—no simctl path juggling needed
maestro test .maestro/smoke.yaml
