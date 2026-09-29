#!/usr/bin/env bash
# check-event-shape.sh
# Verifies that every env.events().publish() call site across all contracts
# uses a centralized events::event_*() constructor (never inline Symbol::new).
# Also verifies that every constructor-exported topic appears in EVENT_TOPICS.md.
# Issue #1118: additionally verifies that every vault publish() call site
# includes the version constructor (events::event_version_v1) at topic[1].
#
# Exit 0 = all checks pass. Exit 1 = any check fails.
set -euo pipefail

SCHEMA="docs/EVENT_TOPICS.md"

if [[ ! -f "$SCHEMA" ]]; then
  echo "ERROR: $SCHEMA not found (run from repo root)" >&2
  exit 1
fi

FAIL=0

check_contract() {
  local contract_name="$1"
  local lib="contracts/${contract_name}/src/lib.rs"
  local events="contracts/${contract_name}/src/events.rs"

  if [[ ! -f "$lib" ]]; then
    echo "WARN: $lib not found, skipping $contract_name" >&2
    return
  fi

  echo "=== $contract_name Event Shape Check ==="

  # 1. Verify no inline Symbol::new in publish call sites
  local INLINE_COUNT
  INLINE_COUNT=$(grep -cP 'env\.events\(\)\.publish\(\(.*Symbol::new' "$lib" 2>/dev/null || true)
  if [[ "$INLINE_COUNT" -gt 0 ]]; then
    echo "FAIL: $contract_name has $INLINE_COUNT inline Symbol::new in publish() calls"
    grep -nP 'env\.events\(\)\.publish\(\(.*Symbol::new' "$lib" || true
    FAIL=1
  else
    echo "OK: no inline Symbol::new in publish() calls"
  fi

  # 2. Verify every event constructor in events.rs has a snapshot test
  if [[ -f "$events" ]]; then
    local CTOR_COUNT
    CTOR_COUNT=$(grep -cP 'pub fn event_\w+' "$events" || true)
    local TESTED_SYMBOLS
    TESTED_SYMBOLS=$(grep -oP 'Symbol::new\(&env,\s*"\K[a-z_]+' "$events" | sort -u | wc -l)
    if [[ "$CTOR_COUNT" -ne "$TESTED_SYMBOLS" ]]; then
      echo "FAIL: $contract_name has $CTOR_COUNT constructors but tests cover $TESTED_SYMBOLS unique symbols"
      FAIL=1
    else
      echo "OK: $CTOR_COUNT constructors match $TESTED_SYMBOLS tested symbols"
    fi
  fi

  # 3. Verify every constructor-exported topic appears in EVENT_TOPICS.md
  if [[ -f "$events" ]]; then
    mapfile -t TOPIC_STRINGS < <(
      grep -oP 'Symbol::new\(env,\s*"\K[a-z_]+' "$events" | sort -u
    )
    mapfile -t SCHEMA_TOPICS < <(
      grep -oP '^\|\s*\d+\s*\|\s*`\K[a-z_]+(?=`)' "$SCHEMA" | sort -u
    )

    local MISSING=()
    for topic in "${TOPIC_STRINGS[@]}"; do
      if ! printf '%s\n' "${SCHEMA_TOPICS[@]}" | grep -qx "$topic"; then
        MISSING+=("$topic")
      fi
    done

    if [[ ${#MISSING[@]} -gt 0 ]]; then
      echo "FAIL: the following $contract_name topics are in events.rs but not in EVENT_TOPICS.md:"
      for m in "${MISSING[@]}"; do
        echo "  - $m"
      done
      FAIL=1
    else
      echo "OK: all ${#TOPIC_STRINGS[@]} topics documented in EVENT_TOPICS.md"
    fi
  fi

  echo ""
}

# Issue #1118: verify every vault publish() call includes event_version_v1.
# Strategy: count publish() calls vs publish() calls that also reference
# event_version_v1 in the same multi-line block (up to 6 lines ahead).
check_vault_version_topics() {
  local lib="contracts/vault/src/lib.rs"
  echo "=== vault Version-Topic Check (Issue #1118) ==="

  # Count publish blocks that lack event_version_v1.
  # Each publish( opens a tuple; we collect lines until the closing );
  # and check whether event_version_v1 appears in those lines.
  local MISSING
  MISSING=$(python3 - "$lib" <<'PYEOF'
import re, sys

text = open(sys.argv[1]).read()
lines = text.splitlines()

violations = []
i = 0
while i < len(lines):
    line = lines[i]
    if 'env.events().publish(' in line:
        # Collect this block until we see ); at start of a line (end of call)
        block_lines = [line]
        j = i + 1
        while j < len(lines) and j < i + 20:
            block_lines.append(lines[j])
            if lines[j].strip().startswith(');'):
                break
            j += 1
        block = '\n'.join(block_lines)
        if 'event_version_v1' not in block:
            violations.append(f"  line {i+1}: {line.strip()[:80]}")
    i += 1

for v in violations:
    print(v)
print(len(violations))
PYEOF
  )

  local COUNT
  COUNT=$(echo "$MISSING" | tail -1)
  local DETAILS
  DETAILS=$(echo "$MISSING" | head -n -1)

  if [[ "$COUNT" -gt 0 ]]; then
    echo "FAIL: $COUNT vault publish() call(s) missing event_version_v1:"
    echo "$DETAILS"
    FAIL=1
  else
    echo "OK: all vault publish() calls include event_version_v1"
  fi
  echo ""
}

check_contract "vault"
check_contract "settlement"
check_contract "revenue_pool"
check_vault_version_topics

if [[ "$FAIL" -ne 0 ]]; then
  echo "FAILED: some contracts have undocumented or unversioned events. See above."
  exit 1
fi

echo "OK: all event constructors are centralized, tested, and documented."
exit 0
