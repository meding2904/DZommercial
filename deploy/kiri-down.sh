#!/usr/bin/env bash
set -euo pipefail

for name in azahar-web azahar-api azahar-db; do
	if command -v kiri >/dev/null 2>&1; then
		if kiri ps -a --format '{{.Names}}' 2>/dev/null | grep -qx "$name"; then
			echo "Removing $name..."
			kiri rm -f "$name" >/dev/null
		fi
	fi
done

echo "Containers removed. The azahar-db volume is kept."
