#!/bin/zsh
# Usage: ./run-scenarios.sh   (needs Screen Recording permission for the host terminal/app)
export PATH=~/.cargo/bin:$PATH
cd "$(dirname "$0")"
export LOWK_SPIKE_LOG=$PWD/run.log LOWK_SPIKE_CTL=$PWD/spike.ctl
[ -x probe/probe ] || swiftc -swift-version 5 -o probe/probe probe/probe.swift
(cd app/src-tauri && cargo build) || exit 1
pkill -f stealth-spike; pkill -f "probe fsdummy"; sleep 1; rm -f run.log spike.ctl
nohup app/src-tauri/target/debug/stealth-spike > app.out 2>&1 &
sleep 4
ctl() { echo "$1" > spike.ctl; sleep 1.5; }
P() { ./probe/probe results "$1"; }
ctl "main-hide";                 P 1_overlay_alone
ctl "main-focus";                P 2_overlay_plus_main
ctl "main-hide"; ctl "main-focus"; P 3_after_main_toggle
ctl "overlay-move 300 400";      P 4_after_overlay_move
ctl "overlay-toggle-exclusion off"; P 5_NEGATIVE_CONTROL_exclusion_off
ctl "overlay-toggle-exclusion on";  P 6_exclusion_back_on
ctl "overlay-hide";              P 7_overlay_hidden_panic
ctl "overlay-show"
# fullscreen space: dummy blue fullscreen window
./probe/probe fsdummy & FS=$!; sleep 6; P 8_over_fullscreen_space
ctl "overlay-toggle-exclusion off"; P 8b_NEGATIVE_CONTROL_fullscreen_exclusion_off
ctl "overlay-toggle-exclusion on"; kill $FS
ctl "quit"
