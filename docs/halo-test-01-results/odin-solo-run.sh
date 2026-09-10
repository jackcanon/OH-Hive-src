#!/bin/bash
cd ~/halo/solo
B=~/halo/llama.cpp/build/bin/llama-bench
BL=~/.ollama/models/blobs
S=$(date +%Y%m%d-%H%M)
LOG=odin-solo-$S.log; CSV=odin-solo-$S-telemetry.csv
echo "ts,cpu_speed_limit,battery_temp_c,batt_amperage_mA,batt_voltage_mV,load1,pages_free_gb,top_proc" > $CSV
( while true; do
  lim=$(pmset -g therm 2>/dev/null | awk -F= "/CPU_Speed_Limit/{gsub(/ /,\"\",\$2);print \$2}"); [ -z "$lim" ] && lim=100
  bt=$(ioreg -rn AppleSmartBattery | awk -F= "/\"Temperature\"/{gsub(/ /,\"\",\$2);printf \"%.1f\", \$2/100}")
  amp=$(ioreg -rn AppleSmartBattery | awk -F= "/\"InstantAmperage\"/{gsub(/ /,\"\",\$2);print \$2}")
  volt=$(ioreg -rn AppleSmartBattery | awk -F= "/\"Voltage\"/{gsub(/ /,\"\",\$2);print \$2}")
  l1=$(sysctl -n vm.loadavg | awk "{print \$2}")
  fr=$(vm_stat | awk "/Pages free/{gsub(/\\./,\"\",\$3);printf \"%.1f\", \$3*16384/1e9}")
  tp=$(ps -Ao %cpu,comm -r | sed -n 2p | awk "{printf \"%s:%s\", \$1, \$2}" | sed "s#.*/##")
  echo "$(date +%H:%M:%S),$lim,$bt,$amp,$volt,$l1,$fr,$tp" >> $CSV
  sleep 10
done ) & TEL=$!
run() { echo "=== $1 ($2) :: $3" >> $LOG; echo "START $(date +%H:%M:%S)" >> $LOG; $B -m $2 $3 -o md 2>&1 | grep -E "^\||error|failed" >> $LOG; echo "END $(date +%H:%M:%S)" >> $LOG; echo >> $LOG; }
STD="-p 512 -n 128 -r 3"
run gemma3:4b        $BL/sha256-aeda25e63ebd698fab8638ffb778e68bed908b960d39d0becc650fa981609d25 "$STD"
run llama3.1:8b      $BL/sha256-667b0c1932bc6ffc593ed1d03f895bf2dc8dc6df21db3042284a6f4416b06a29 "$STD"
run qwen3.5:9b       $BL/sha256-dec52a44569a2a25341c4e4d3fee25846eed4f6f0b936278e3a3c900bb99d37c "$STD"
run gemma4:12b-qat   $BL/sha256-faff1a63667fac17ac5e777f47114688fcefea96e220e211aaa8d62c2c4561f1 "$STD"
run qwen3:14b        $BL/sha256-a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e "$STD"
run qwen3-14B-Q4KM   ~/halo/Qwen_Qwen3-14B-Q4_K_M.gguf "$STD"
run mistral-small:24b $BL/sha256-102a747c137683e81d431dab05d8f2158df4ab6f162f8f9019425a43d51e0e9f "$STD"
run qwen3.6-21.7GB-EDGE $BL/sha256-d372de8e934898a59e6ccfabc3368474711384d8f1fd4d22d87a3f0a45400cdc "$STD"
SUS="-p 512 -n 1024 -r 3"
run SUSTAINED-qwen3:14b $BL/sha256-a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e "$SUS"
run SUSTAINED-mistral-small:24b $BL/sha256-102a747c137683e81d431dab05d8f2158df4ab6f162f8f9019425a43d51e0e9f "$SUS"
kill $TEL; echo ALL_DONE >> $LOG
