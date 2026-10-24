#!/usr/bin/env bash
# Spike recording session: records accelerometer + lid angle while prompting the user through scenarios.
# Usage: tools/record-spike.sh [out_dir]
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-fixtures/traces/spike-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$OUT"
cargo build -q -p lid-probe -p accel-probe

# segment: label seconds instruction
SEGMENTS=(
  "idle|6|保持不動，手離開電腦"
  "typing|8|正常速度打字（隨便打）"
  "trackpad|6|點按觸控板幾下"
  "slap_light|8|輕拍掌托旁邊 3 次（間隔約 2 秒）"
  "slap_medium|8|中等力道拍 3 次"
  "slap_hard|8|用力拍 3 次（別弄壞電腦）"
  "slap_combo|6|快速連拍 5 下"
  "lid_move|12|慢慢把螢幕往下闔到約 30°，再推開到最大角度"
  "idle_end|4|保持不動"
)
TOTAL=0
for s in "${SEGMENTS[@]}"; do IFS='|' read -r _ sec _ <<<"$s"; TOTAL=$((TOTAL + sec)); done
TOTAL=$((TOTAL + 2))

read -r -p "準備好後按 Enter 開始錄製（約 ${TOTAL}s）… " _

./target/debug/accel-probe --enable --ndjson --dump 0 --secs "$TOTAL" >"$OUT/accel.ndjson" 2>"$OUT/accel.stderr" &
APID=$!
./target/debug/lid-probe --mode input --ndjson --secs "$TOTAL" >"$OUT/lid.ndjson" 2>"$OUT/lid.stderr" &
LPID=$!
sleep 1

LABELS="$OUT/labels.ndjson"
: >"$LABELS"
echo "錄製中（共約 ${TOTAL}s）。請依照提示操作："
for s in "${SEGMENTS[@]}"; do
  IFS='|' read -r label sec msg <<<"$s"
  start=$(python3 -c 'import time;print(f"{time.time()*1000:.1f}")')
  printf '\n▶ [%s] %s（%ss）\n' "$label" "$msg" "$sec"
  for ((i = sec; i > 0; i--)); do printf '  %2d\r' "$i"; sleep 1; done
  end=$(python3 -c 'import time;print(f"{time.time()*1000:.1f}")')
  echo "{\"label\":\"$label\",\"start_unix_ms\":$start,\"end_unix_ms\":$end}" >>"$LABELS"
done
wait $APID $LPID || true
echo
echo "完成！輸出：$OUT"
wc -l "$OUT"/*.ndjson
