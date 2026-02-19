#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

SERVER_PORT="${LMLANG_PORT:-3000}"
EXTRA_PORTS="${LMLANG_FREE_PORTS:-}"

PORTS=("${SERVER_PORT}")
if [[ -n "${EXTRA_PORTS}" ]]; then
  IFS=',' read -r -a EXTRA <<< "${EXTRA_PORTS}"
  for p in "${EXTRA[@]}"; do
    p="$(echo "${p}" | xargs)"
    [[ -n "${p}" ]] && PORTS+=("${p}")
  done
fi

echo "Freeing ports: ${PORTS[*]}"
for port in "${PORTS[@]}"; do
  PIDS="$(lsof -tiTCP:"${port}" -sTCP:LISTEN || true)"
  if [[ -n "${PIDS}" ]]; then
    echo "Port ${port} in use by PID(s): ${PIDS}"
    kill ${PIDS} || true
    sleep 1

    REMAINING="$(lsof -tiTCP:"${port}" -sTCP:LISTEN || true)"
    if [[ -n "${REMAINING}" ]]; then
      echo "Force killing PID(s) on port ${port}: ${REMAINING}"
      kill -9 ${REMAINING} || true
    fi
  fi
done

echo "Starting lmlang-server on port ${SERVER_PORT}..."
exec cargo run -p lmlang-server
