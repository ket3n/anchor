#!/usr/bin/env bash
# Stops benchmark processes when the Claude subscription session usage reaches a cap.
# Usage: quota_guard.sh <cap_percent> <pgrep-pattern>
cap=${1:-50}; pat=${2:-compare.py}
while pgrep -f "$pat" >/dev/null; do
  pct=$(timeout 90 claude -p "/usage" --model sonnet --no-session-persistence </dev/null 2>/dev/null \
        | grep -o "Current session: [0-9]*%" | grep -o "[0-9]*")
  echo "$(date +%H:%M) session usage ${pct:-?}%"
  if [ -n "$pct" ] && [ "$pct" -ge "$cap" ]; then
    echo "cap ${cap}% reached: stopping benchmark"; pkill -f "$pat"; exit 0
  fi
  sleep 120
done
echo "benchmark finished under the cap"
