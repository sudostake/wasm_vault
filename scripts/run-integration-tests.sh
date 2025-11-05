#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

if [[ ! -d tests ]]; then
  echo "No integration tests found (missing tests/ directory)." >&2
  exit 0
fi

TEST_FILES=()
while IFS= read -r file; do
  TEST_FILES+=("$file")
done < <(find tests -maxdepth 1 -type f -name "*.rs" | sort)

if [[ ${#TEST_FILES[@]} -eq 0 ]]; then
  echo "No integration test files found in tests/." >&2
  exit 0
fi

status=0
for file in "${TEST_FILES[@]}"; do
  test_name="$(basename "${file%.rs}")"
  echo "Running integration test target: ${test_name}"
  if ! cargo test --test "${test_name}" "$@"; then
    status=$?
    break
  fi
done

exit "${status}"
