#!/usr/bin/env bash
set -euo pipefail

LOCAL="http://localhost:8080/api/v1/events"
DEPLOYED="https://administration.info.intersectmbo.org/api/v1/events"
LIMIT=100

LOCAL_FILE=$(mktemp)
DEPLOYED_FILE=$(mktemp)
trap 'rm -f "$LOCAL_FILE" "$DEPLOYED_FILE"' EXIT

fetch_all() {
  local url=$1 outfile=$2 page=1
  > "$outfile"
  while true; do
    resp=$(curl -s "${url}?limit=${LIMIT}&page=${page}")
    count=$(echo "$resp" | jq '.data | length')
    if [ "$count" -eq 0 ]; then break; fi
    echo "$resp" | jq -r '.data[] | [.tx_hash, .event_type, (.slot // ""), (.project_id // "")] | @csv' >> "$outfile"
    if [ "$count" -lt "$LIMIT" ]; then break; fi
    page=$((page + 1))
  done
  echo "Fetched $(wc -l < "$outfile" | tr -d ' ') events from $url" >&2
}

echo "Fetching local events..." >&2
fetch_all "$LOCAL" "$LOCAL_FILE"

echo "Fetching deployed events..." >&2
fetch_all "$DEPLOYED" "$DEPLOYED_FILE"

# Sort both files for comparison
sort "$LOCAL_FILE" > "${LOCAL_FILE}.sorted"
sort "$DEPLOYED_FILE" > "${DEPLOYED_FILE}.sorted"

OUTPUT="diverging_events.csv"
echo "tx_hash,event_type,slot,project_id,source" > "$OUTPUT"

# Lines only in local
comm -23 "${LOCAL_FILE}.sorted" "${DEPLOYED_FILE}.sorted" | while IFS= read -r line; do
  echo "${line},\"local_only\""
done >> "$OUTPUT"

# Lines only in deployed
comm -13 "${LOCAL_FILE}.sorted" "${DEPLOYED_FILE}.sorted" | while IFS= read -r line; do
  echo "${line},\"deployed_only\""
done >> "$OUTPUT"

total=$(tail -n +2 "$OUTPUT" | wc -l | tr -d ' ')
local_only=$(grep -c 'local_only' "$OUTPUT" || true)
deployed_only=$(grep -c 'deployed_only' "$OUTPUT" || true)

echo ""
echo "Results written to $OUTPUT"
echo "  Total divergences: $total"
echo "  Local only: $local_only"
echo "  Deployed only: $deployed_only"

rm -f "${LOCAL_FILE}.sorted" "${DEPLOYED_FILE}.sorted"
