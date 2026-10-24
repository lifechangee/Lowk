# Soak: 2 h of Russian speech through the whole session path

Run with `just soak` (`lowk --selfcheck-soak`, `crates/lowk-app/src/soak.rs`); not part of `just ci`. Silent and headless: the 24 bench
clips (`spikes/stt-bench/audio`, our own recordings and synthesized speech, ~22 min with 2 s gaps, 1998 reference words) are looped
as the **System** stream for 7200 s in **real time** through the real capture ring -> resampler -> session -> local STT worker
(Nemotron 3.5, R=6, plus Whisper background correction, product defaults) -> transcript -> vault `.md` -> search index.
No sound, no permission prompts, no cloud. Date 2026-10-24, MacBook (Apple Silicon, Metal), release build, worktree `wp/soak`.

## Verdict

| area | result |
|---|---|
| audio fed / session | 7200 s fed in 7214 s wall, clean stop |
| worker restarts, STT errors | **0 / 0** |
| ring overruns / dropped audio | **0** |
| transcript completeness | every 10-min window recognised 0.90-1.17 of the reference words; longest silence between finals 30.6 s (longest clip 96 s); bus 10 675 words == `.md` 10 675 words (981 utterances, 890 lines after merging) |
| RSS | app 278 -> 289 MB (flat); workers 2835 -> 1682 MB (fell, no growth) |
| `.md` | 137 KB after 2 h (~69 KB/h, linear); written every ~20 s (286 materialisations) |
| search index | re-index of the growing call 0.3-0.4 s at every probe (15 -> 97 chunks); keyword search 0-3 ms |
| latency drift | **see caveat**: only under CPU contention |

## Caveat: machine contention, not drift

During window 0-10 min (first minutes), 10-60 min and 100-110 min the same Mac ran heavy parallel cargo / Playwright builds of other
agents (load average 22-38 on 10 cores, the table shows the mean per window; window 0 has no load log but backlog was already
visible at 13 min). The engine then falls behind (backlog up to 35 s), the estimator reports word-to-screen of 24-60 s p50 for those windows, and recovers by itself
afterwards, with no audio dropped and no restart. On the quiet windows (load <= 4: 60-100 min) the pipeline sits at
**p50 574-800 ms / p95 1.8-2.6 s**, the same as the 20-clip replay in docs/DECISIONS-M1.md section 4 (740 / 1907 ms).
So this run proves the long-session behaviour (memory, completeness, recovery, file and index growth) and shows that Lowk degrades
gracefully (queues, never drops) when the machine is overloaded; it does **not** prove a latency target on a loaded machine.
For a clean latency number repeat `just soak` on an otherwise idle machine.

The first version of the drift check compared first and last 15-minute windows and failed on exactly these spikes (p50 trend -6876 ms/h,
p95 12.7 s -> 30.8 s). It is now robust: median of the 10-min window p50s, second half vs first half (+300 ms allowed), and every
window and sample records the machine load average. For this run, from the table below: first half median 3118 ms, second half 680 ms -> PASS (no drift;
the opposite of drift). The run itself was executed with the earlier check, so its raw output below still prints that one FAIL.

## Numbers

# Soak run

- audio fed: **7200 s (2.00 h)** as the System stream, looped from 24 bench clips (1338 s loop, 1998 reference words/loop), speed x1
- wall time: 7214 s; correction (Whisper): on; engine: nemotron-local
- ring: real capture pipeline (48 kHz ring -> resampler -> frames); AEC off (no Me stream)

- clips: 01_me_n (90 s), 01_me_q (96 s), 01_them_n (40 s), 01_them_q (42 s), 02_me_n (76 s), 02_me_q (64 s), 02_them_n (33 s), 02_them_q (34 s), 03_me_n (64 s), 03_me_q (65 s), 03_them_n (22 s), 03_them_q (20 s), 04_me_n (55 s), 04_me_q (59 s), 04_them_n (40 s), 04_them_q (51 s), 05_me_n (60 s), 05_me_q (59 s), 05_them_n (36 s), 05_them_q (40 s), fleurs_ru_1 (61 s), fleurs_ru_2 (78 s), fleurs_ru_3 (82 s), synth_codeswitch_ru_en (21 s)

## Checks

| check | result | detail |
|---|---|---|
| audio fed to the end | PASS | 7200 s of 7200 s in 7214 s wall |
| session stops cleanly | PASS | Ok(()) |
| STT worker never died or errored | PASS | 0 restarts, 0 errors None |
| no ring overruns / dropped audio | PASS | 0 samples in 0 events |
| no transcript gaps (every 10-min window >= 70% of the reference words, no silence > longest clip + 20 s) | PASS | min window ratio 0.90, max gap between finals 30.6 s (longest clip 96 s) |
| everything the bus finalised reached the .md | PASS | bus 10675 words / 981 utterances vs .md 10675 words / 890 lines |
| RSS does not grow (< 25 MB/h trend after 15 min, app + workers) | PASS | -202.8 MB/h |
| latency does not drift (p50 trend < 100 ms/h; last window p95 <= 1.5x first + 300 ms) | **FAIL** | p50 trend -6876 ms/h; p95 first 12746 -> last 30787 ms |

## Memory

- app RSS 278 -> 289 MB; workers 2835 -> 1682 MB (first sample after 2 min -> last)
- trend after 15 min: -202.8 MB/h (app + workers)

## Word-to-screen latency (estimator of docs/DECISIONS-M1.md section 4, +-0.3 s)

- first 900 s window (from 1 min): n=1308 p50 1676 ms p95 12746 ms
- last 900 s window: n=1348 p50 962 ms p95 30787 ms
- p50 trend: -6876 ms/h
- whole run: n=10797 p50 1238 ms p95 45348 ms

| window (audio min) | words | p50 ms | p95 ms | words fed (ref) | words recognised | ratio | load avg (mean) |
|---|---|---|---|---|---|---|---|
| 0-10 | 849 | 941 | 3180 | 890 | 844 | 0.95 | n/a (>30, see below) |
| 10-20 | 953 | 3671 | 13760 | 933 | 926 | 0.99 | 38 |
| 20-30 | 777 | 24281 | 56643 | 881 | 871 | 0.99 | 31 |
| 30-40 | 1000 | 32109 | 87158 | 896 | 916 | 1.02 | 34 |
| 40-50 | 852 | 2564 | 32874 | 901 | 882 | 0.98 | 26 |
| 50-60 | 950 | 1719 | 24576 | 870 | 903 | 1.04 | 22 |
| 60-70 | 957 | 744 | 2151 | 921 | 907 | 0.98 | 4 |
| 70-80 | 875 | 574 | 1822 | 852 | 870 | 1.02 | 2 |
| 80-90 | 928 | 800 | 2575 | 997 | 903 | 0.91 | 2 |
| 90-100 | 829 | 589 | 1940 | 768 | 837 | 1.09 | 2 |
| 100-110 | 916 | 3558 | 23874 | 1037 | 937 | 0.90 | 17 |
| 110-120 | 909 | 617 | 31498 | 750 | 879 | 1.17 | 12 |

(last window is partial; ratio compares finalised words with the words of clips fully fed in that window)

## Counts

- utterances on the bus: 981 (981 non-empty), live finals 981, corrected revisions 531, partial events 13120
- finals pushed into the call journal: 981; journal materialisations: 286
- .md: 137 KB, 890 lines, 10675 words
- engine: Some(SttDiagnostics { engine: "nemotron-local", lookahead_ms: 400, streams: [StreamDiag { stream: Me, emit_lag: Percentiles { samples: 0, p50_ms: 0, p95_ms: 0, max_ms: 0 }, final_lag: Percentiles { samples: 0, p50_ms: 0, p95_ms: 0, max_ms: 0 }, backlog_ms: 0, restarts: 0 }, StreamDiag { stream: System, emit_lag: Percentiles { samples: 2000, p50_ms: 78, p95_ms: 29281, max_ms: 42042 }, final_lag: Percentiles { samples: 980, p50_ms: 447, p95_ms: 49421, max_ms: 90880 }, backlog_ms: 0, restarts: 0 }], corrected: 530, correction_skipped: 403, correction_lag: Percentiles { samples: 573, p50_ms: 3319, p95_ms: 5657, max_ms: 29277 } })

## Search index

| audio s | .md KB | chunks | update s | search ms |
|---|---|---|---|---|
| 1200 | 22 | 15 | 0.3 | 2 |
| 2400 | 46 | 32 | 0.4 | 1 |
| 3601 | 70 | 50 | 0.3 | 1 |
| 4801 | 92 | 65 | 0.3 | 0 |
| 6001 | 114 | 81 | 0.3 | 2 |
| 7200 | 137 | 97 | 0.3 | 3 |

## Series

| wall s | audio s | app MB | workers MB | .md KB | p50 ms | p95 ms | words | restarts | backlog ms | emit p95 ms | dropped |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 61 | 60 | 449 | 2834 | 1 | 581 | 1876 | 99 | 0 | 280 | 924 | 0 |
| 120 | 120 | 278 | 2835 | 2 | 1067 | 1831 | 95 | 0 | 540 | 989 | 0 |
| 181 | 180 | 283 | 2831 | 3 | 768 | 3443 | 79 | 0 | 280 | 2402 | 0 |
| 241 | 240 | 285 | 2835 | 4 | 538 | 2490 | 66 | 0 | 1040 | 2402 | 0 |
| 301 | 300 | 285 | 2831 | 5 | 916 | 3414 | 70 | 0 | 240 | 2485 | 0 |
| 361 | 360 | 285 | 2831 | 6 | 681 | 3052 | 75 | 0 | 40 | 2485 | 0 |
| 421 | 420 | 285 | 2835 | 8 | 1823 | 4251 | 97 | 0 | 720 | 2661 | 0 |
| 481 | 480 | 285 | 2835 | 8 | 1172 | 3253 | 75 | 0 | 20 | 2646 | 0 |
| 541 | 540 | 285 | 2824 | 9 | 469 | 4187 | 71 | 0 | 520 | 2709 | 0 |
| 601 | 600 | 285 | 2824 | 10 | 1209 | 4348 | 79 | 0 | 160 | 2853 | 0 |
| 661 | 660 | 285 | 2754 | 12 | 3102 | 5017 | 74 | 0 | 2600 | 3153 | 0 |
| 720 | 720 | 285 | 2754 | 13 | 2328 | 4765 | 105 | 0 | 220 | 3322 | 0 |
| 781 | 780 | 285 | 2754 | 14 | 9777 | 16242 | 82 | 0 | 16700 | 4340 | 0 |
| 841 | 840 | 285 | 2758 | 16 | 6863 | 16377 | 118 | 0 | 1919 | 12466 | 0 |
| 901 | 900 | 286 | 2755 | 17 | 2067 | 3624 | 77 | 0 | 2080 | 12091 | 0 |
| 960 | 960 | 286 | 2740 | 18 | 3224 | 7728 | 63 | 0 | 6200 | 11940 | 0 |
| 1020 | 1020 | 286 | 2739 | 19 | 3442 | 6483 | 106 | 0 | 5700 | 11626 | 0 |
| 1081 | 1080 | 286 | 2758 | 20 | 6992 | 11317 | 105 | 0 | 1040 | 10520 | 0 |
| 1141 | 1140 | 286 | 2755 | 21 | 2501 | 5512 | 81 | 0 | 6900 | 10342 | 0 |
| 1200 | 1200 | 277 | 2727 | 22 | 6919 | 10399 | 84 | 0 | 200 | 10342 | 0 |
| 1261 | 1260 | 281 | 2727 | 23 | 753 | 1603 | 82 | 0 | 200 | 10342 | 0 |
| 1321 | 1320 | 286 | 2730 | 24 | 647 | 12634 | 80 | 0 | 180 | 10342 | 0 |
| 1381 | 1380 | 289 | 2730 | 26 | 4697 | 8686 | 75 | 0 | 11900 | 10559 | 0 |
| 1441 | 1440 | 289 | 2715 | 27 | 22281 | 28487 | 59 | 0 | 31440 | 15201 | 0 |
| 1501 | 1500 | 278 | 1669 | 27 | 36659 | 41901 | 58 | 0 | 45840 | 28792 | 0 |
| 1561 | 1560 | 272 | 1670 | 29 | 50192 | 53779 | 67 | 0 | 55420 | 45815 | 0 |
| 1621 | 1620 | 169 | 1631 | 30 | 47382 | 57163 | 114 | 0 | 21800 | 51836 | 0 |
| 1681 | 1680 | 171 | 1631 | 31 | 24341 | 32910 | 66 | 0 | 24120 | 51836 | 0 |
| 1741 | 1740 | 176 | 1631 | 32 | 26507 | 27762 | 67 | 0 | 37240 | 51836 | 0 |
| 1801 | 1800 | 177 | 1631 | 33 | 42045 | 57046 | 34 | 0 | 65360 | 53003 | 0 |
| 1861 | 1860 | 177 | 1536 | 34 | 80884 | 86562 | 62 | 0 | 87780 | 56258 | 0 |
| 1921 | 1920 | 186 | 1537 | 35 | 87488 | 90844 | 89 | 0 | 63800 | 80921 | 0 |
| 1981 | 1980 | 194 | 1538 | 37 | 45072 | 61348 | 126 | 0 | 35340 | 80921 | 0 |
| 2041 | 2040 | 202 | 1538 | 38 | 40810 | 47171 | 58 | 0 | 47200 | 80921 | 0 |
| 2100 | 2100 | 211 | 1538 | 39 | 54676 | 56059 | 60 | 0 | 59400 | 80921 | 0 |
| 2160 | 2160 | 220 | 1570 | 42 | 25632 | 59378 | 209 | 0 | 1040 | 80921 | 0 |
| 2220 | 2220 | 227 | 1570 | 43 | 1026 | 2437 | 85 | 0 | 580 | 80921 | 0 |
| 2281 | 2280 | 234 | 1567 | 44 | 704 | 3508 | 81 | 0 | 3380 | 80921 | 0 |
| 2340 | 2340 | 234 | 1567 | 45 | 2156 | 4022 | 95 | 0 | 1780 | 80921 | 0 |
| 2401 | 2400 | 237 | 1571 | 46 | 1203 | 3922 | 74 | 0 | 520 | 80921 | 0 |
| 2461 | 2460 | 243 | 1571 | 47 | 1115 | 3671 | 102 | 0 | 50 | 80921 | 0 |
| 2521 | 2520 | 250 | 1583 | 48 | 1613 | 5482 | 75 | 0 | 5760 | 80921 | 0 |
| 2580 | 2580 | 259 | 1568 | 50 | 2895 | 8598 | 88 | 0 | 2080 | 80921 | 0 |
| 2641 | 2640 | 266 | 1568 | 51 | 1209 | 13428 | 74 | 0 | 100 | 80921 | 0 |
| 2701 | 2700 | 270 | 1570 | 52 | 625 | 3203 | 83 | 0 | 460 | 80921 | 0 |
| 2760 | 2760 | 273 | 1570 | 53 | 1148 | 3545 | 81 | 0 | 180 | 80921 | 0 |
| 2821 | 2820 | 277 | 1570 | 54 | 1951 | 4804 | 68 | 0 | 4180 | 80921 | 0 |
| 2880 | 2880 | 279 | 1570 | 55 | 6487 | 9492 | 47 | 0 | 16320 | 72808 | 0 |
| 2940 | 2940 | 279 | 1570 | 56 | 20462 | 26570 | 50 | 0 | 29599 | 58346 | 0 |
| 3001 | 3000 | 280 | 1559 | 57 | 33036 | 34674 | 55 | 0 | 37380 | 54237 | 0 |
| 3061 | 3060 | 282 | 1573 | 59 | 17365 | 45670 | 127 | 0 | 1260 | 54027 | 0 |
| 3121 | 3120 | 284 | 1570 | 60 | 988 | 2513 | 91 | 0 | 620 | 45710 | 0 |
| 3180 | 3180 | 286 | 1570 | 61 | 692 | 1742 | 54 | 0 | 220 | 34267 | 0 |
| 3241 | 3240 | 286 | 1585 | 62 | 741 | 7072 | 73 | 0 | 4260 | 32366 | 0 |
| 3301 | 3300 | 286 | 1772 | 63 | 7311 | 14999 | 101 | 0 | 520 | 32366 | 0 |
| 3360 | 3360 | 286 | 1769 | 64 | 2529 | 13346 | 54 | 0 | 12000 | 32366 | 0 |
| 3421 | 3420 | 286 | 940 | 66 | 1771 | 11089 | 125 | 0 | 5620 | 32366 | 0 |
| 3481 | 3480 | 287 | 1770 | 68 | 10491 | 18526 | 118 | 0 | 1320 | 32366 | 0 |
| 3541 | 3540 | 287 | 1771 | 69 | 1602 | 4546 | 89 | 0 | 100 | 32366 | 0 |
| 3601 | 3600 | 287 | 1783 | 70 | 794 | 2020 | 85 | 0 | 380 | 32366 | 0 |
| 3660 | 3660 | 287 | 1771 | 71 | 717 | 2141 | 98 | 0 | 780 | 32366 | 0 |
| 3720 | 3720 | 287 | 1771 | 72 | 975 | 1937 | 103 | 0 | 240 | 32366 | 0 |
| 3780 | 3780 | 287 | 1767 | 72 | 784 | 1873 | 86 | 0 | 60 | 32366 | 0 |
| 3840 | 3840 | 287 | 1767 | 74 | 617 | 2532 | 68 | 0 | 320 | 32366 | 0 |
| 3901 | 3900 | 287 | 1769 | 75 | 1097 | 2108 | 99 | 0 | 160 | 32366 | 0 |
| 3961 | 3960 | 287 | 1769 | 77 | 1083 | 13165 | 67 | 0 | 300 | 32366 | 0 |
| 4021 | 4020 | 288 | 1769 | 77 | 407 | 1282 | 95 | 0 | 420 | 32366 | 0 |
| 4081 | 4080 | 288 | 1769 | 79 | 497 | 1364 | 91 | 0 | 180 | 12586 | 0 |
| 4141 | 4140 | 288 | 1769 | 80 | 752 | 1651 | 89 | 0 | 460 | 11744 | 0 |
| 4201 | 4200 | 288 | 1769 | 81 | 497 | 1046 | 71 | 0 | 440 | 11744 | 0 |
| 4261 | 4260 | 288 | 1769 | 82 | 404 | 1328 | 67 | 0 | 560 | 11744 | 0 |
| 4320 | 4320 | 288 | 1773 | 83 | 522 | 1428 | 75 | 0 | 60 | 10346 | 0 |
| 4380 | 4380 | 288 | 1769 | 84 | 382 | 1510 | 80 | 0 | 660 | 9301 | 0 |
| 4440 | 4440 | 288 | 1769 | 85 | 955 | 2337 | 89 | 0 | 160 | 6817 | 0 |
| 4500 | 4500 | 288 | 1773 | 86 | 680 | 2064 | 70 | 0 | 760 | 6817 | 0 |
| 4560 | 4560 | 288 | 1773 | 87 | -215 | 1206 | 83 | 0 | 2000 | 1195 | 0 |
| 4620 | 4620 | 288 | 1769 | 88 | 623 | 2330 | 81 | 0 | 40 | 967 | 0 |
| 4680 | 4680 | 288 | 1784 | 89 | 924 | 1843 | 76 | 0 | 960 | 939 | 0 |
| 4741 | 4740 | 288 | 1769 | 90 | 642 | 1575 | 106 | 0 | 140 | 927 | 0 |
| 4801 | 4800 | 288 | 1769 | 92 | 611 | 1400 | 101 | 0 | 240 | 911 | 0 |
| 4860 | 4860 | 288 | 1773 | 93 | 661 | 1816 | 88 | 0 | 200 | 916 | 0 |
| 4920 | 4920 | 288 | 1769 | 94 | 716 | 1878 | 84 | 0 | 780 | 927 | 0 |
| 4980 | 4980 | 288 | 1773 | 95 | 753 | 5486 | 95 | 0 | 1060 | 934 | 0 |
| 5040 | 5040 | 288 | 1769 | 96 | 958 | 2416 | 90 | 0 | 900 | 947 | 0 |
| 5100 | 5100 | 288 | 1773 | 97 | 1065 | 2079 | 95 | 0 | 340 | 957 | 0 |
| 5160 | 5160 | 288 | 1769 | 98 | 530 | 1365 | 82 | 0 | 320 | 962 | 0 |
| 5221 | 5220 | 288 | 1769 | 99 | 1064 | 2505 | 69 | 0 | 140 | 976 | 0 |
| 5281 | 5280 | 288 | 1769 | 100 | 732 | 1853 | 69 | 0 | 260 | 980 | 0 |
| 5341 | 5340 | 288 | 1769 | 102 | 638 | 13423 | 71 | 0 | 580 | 979 | 0 |
| 5401 | 5400 | 288 | 1769 | 103 | 408 | 2046 | 85 | 0 | 180 | 985 | 0 |
| 5461 | 5460 | 288 | 1769 | 104 | 960 | 1760 | 71 | 0 | 0 | 1005 | 0 |
| 5521 | 5520 | 288 | 1773 | 105 | 551 | 2026 | 82 | 0 | 1700 | 977 | 0 |
| 5580 | 5580 | 288 | 1769 | 106 | 571 | 1712 | 80 | 0 | 60 | 968 | 0 |
| 5640 | 5640 | 288 | 1770 | 107 | 548 | 1731 | 61 | 0 | 640 | 968 | 0 |
| 5700 | 5700 | 288 | 1773 | 108 | 703 | 2022 | 78 | 0 | 1100 | 955 | 0 |
| 5760 | 5760 | 288 | 1773 | 110 | 675 | 1602 | 90 | 0 | 560 | 963 | 0 |
| 5820 | 5820 | 289 | 1773 | 110 | 345 | 1527 | 79 | 0 | 680 | 961 | 0 |
| 5881 | 5880 | 289 | 1770 | 111 | -117 | 1558 | 72 | 0 | 120 | 952 | 0 |
| 5941 | 5940 | 289 | 1769 | 112 | 902 | 3895 | 65 | 0 | 300 | 958 | 0 |
| 6000 | 6000 | 289 | 1773 | 114 | 1031 | 1876 | 80 | 0 | 1120 | 974 | 0 |
| 6061 | 6060 | 289 | 1784 | 115 | 690 | 1508 | 97 | 0 | 800 | 974 | 0 |
| 6121 | 6120 | 289 | 1766 | 116 | 6670 | 12350 | 80 | 0 | 15480 | 1356 | 0 |
| 6180 | 6180 | 288 | 1646 | 118 | 17805 | 24214 | 115 | 0 | 200 | 12956 | 0 |
| 6240 | 6240 | 288 | 1662 | 119 | 760 | 2135 | 72 | 0 | 200 | 12956 | 0 |
| 6301 | 6300 | 288 | 1662 | 120 | 854 | 1840 | 97 | 0 | 80 | 12956 | 0 |
| 6361 | 6360 | 288 | 941 | 121 | 1475 | 6826 | 91 | 0 | 6940 | 12956 | 0 |
| 6421 | 6420 | 288 | 1784 | 122 | 2257 | 8685 | 83 | 0 | 3540 | 12956 | 0 |
| 6481 | 6480 | 288 | 1747 | 122 | 6961 | 10402 | 93 | 0 | 220 | 12956 | 0 |
| 6541 | 6540 | 288 | 1752 | 124 | 4024 | 11405 | 66 | 0 | 12660 | 12956 | 0 |
| 6601 | 6600 | 288 | 1665 | 125 | 20923 | 30728 | 61 | 0 | 32000 | 21222 | 0 |
| 6661 | 6660 | 288 | 1670 | 127 | 29795 | 40124 | 132 | 0 | 80 | 29281 | 0 |
| 6720 | 6720 | 288 | 1682 | 128 | 447 | 1485 | 74 | 0 | 40 | 29281 | 0 |
| 6781 | 6780 | 288 | 1682 | 129 | 776 | 1629 | 94 | 0 | 220 | 29281 | 0 |
| 6840 | 6840 | 288 | 1686 | 130 | 551 | 1785 | 87 | 0 | 40 | 29281 | 0 |
| 6900 | 6900 | 288 | 1686 | 131 | 581 | 1491 | 85 | 0 | 480 | 29281 | 0 |
| 6961 | 6960 | 289 | 1686 | 133 | 474 | 1184 | 74 | 0 | 320 | 29281 | 0 |
| 7020 | 7020 | 289 | 1686 | 133 | 434 | 1543 | 67 | 0 | 1920 | 29281 | 0 |
| 7080 | 7080 | 289 | 1682 | 135 | 604 | 1738 | 85 | 0 | 160 | 29281 | 0 |
| 7141 | 7140 | 289 | 1686 | 136 | 403 | 1648 | 92 | 0 | 0 | 29281 | 0 |
| 7200 | 7200 | 289 | 1686 | 137 | 151 | 1593 | 79 | 0 | 320 | 29281 | 0 |
| 7210 | 7200 | 289 | 1682 | 137 | NaN | NaN | 0 | 0 | 0 | 29281 | 0 |


## How to run

`just soak` (2 h real time, ~2 h wall; run it in the background), `just soak 0.25 3` (15 min at 3x smoke), options after the two
positionals (`--no-correct`, `--sample-secs`, `--probe-mins`, `--keep`). Needs the engine model in
`~/Library/Application Support/Lowk/models` (`LOWK_MODELS`), a `lowk-stt-worker` (`just worker` or `LOWK_STT_WORKER`) and the bench clips
(`LOWK_BENCH_AUDIO`; the m4a files are git-ignored). Report: `target/soak-report.md`, series `target/soak-series.csv`.

Faster than real time is not sustainable: at 8x the engine backlog grows past 30 s and the worker drops audio ("stt backlog: dropping
audio", flagged by the checks); use 1x for the real result. Nothing was downloaded for this run (models were already installed).
