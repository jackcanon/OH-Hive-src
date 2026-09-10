#!/bin/bash
# RPC worker shard-size ladder: Overgaard host, Odin worker. Run from Midgaard (SSH relay only).
# Per-node logic lives in ~/halo/solo/prep.sh + post.sh (Odin) and ~/halo/rung.sh (Overgaard).
R="/Volumes/10TB JBOD/Agents/Claude/Projects/Apps/OH Cloud-src/docs/halo-test-01-results"
OUT="$R/ladder-odin-worker-2.log"
A="ssh -o BatchMode=yes -o ServerAliveInterval=30 asgard"
M14='/Users/jack/halo/models/Qwen_Qwen3-14B-Q4_K_M.gguf'
M32='/Users/jack/halo/models/Qwen_Qwen3-32B-Q8_0.gguf'
echo "# ladder $(date)" > "$OUT"

rung() { # $1 label  $2 model  $3 overgaard_gb  $4 odin_gb
  echo "=== $1 : Odin ${4}GB / Overgaard ${3}GB  $(date +%H:%M:%S)" | tee -a "$OUT"
  $A ssh -o BatchMode=yes odin /Users/jack/halo/solo/prep.sh | tee -a "$OUT"
  $A ssh -o BatchMode=yes -o ServerAliveInterval=30 overgaard /Users/jack/halo/rung.sh "$2" "$3" "$4" | tee -a "$OUT"
  $A ssh -o BatchMode=yes odin /Users/jack/halo/solo/post.sh | tee -a "$OUT"
  echo | tee -a "$OUT"
}

rung "32B-odin6"  "$M32" 28.8 6
rung "32B-odin4"  "$M32" 30.8 4
echo "LADDER_DONE" | tee -a "$OUT"
