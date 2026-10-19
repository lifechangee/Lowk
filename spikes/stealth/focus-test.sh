#!/bin/zsh
# Focus-steal test: overlay show/hide/move/click must not change the frontmost app.
cd "$(dirname "$0")"; export LOWK_SPIKE_LOG=$PWD/run.log LOWK_SPIKE_CTL=$PWD/spike.ctl
front() { lsappinfo info -only name $(lsappinfo front) | tr -d '\n'; echo; }
open -a TextEdit; sleep 2; echo "before:"; front
for c in overlay-hide overlay-show "overlay-move 150 200" "overlay-move 120 180"; do echo "$c" > spike.ctl; sleep 1.5; echo "after $c:"; front; done
echo "click on overlay (CGEvent):"; ${CLK:-/private/tmp/claude-501/clk}; sleep 1; front
