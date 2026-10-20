| engine | set | clips | WER | CER | word lag p50/p90 (s) | onset->1st partial p50 (s) | final after EOS (s) | RTF (compute) | peak RAM MB |
|---|---|---|---|---|---|---|---|---|---|
| nemotron_mlx:bf16-auto | real noisy (_n) | 10 | 0.316 | 0.230 | 0.40/1.54 | 1.39 | 0.14 | 0.15 | 1650 |
| voxtral_mlx:4bit-480ms | real noisy (_n) | 10 | 0.188 | 0.130 | 27.78/31.02 | 1.54 | 28.58 | 1.60 | 3446 |
| whisper_cpp:turbo-q5-ac768-v2 | real noisy (_n) | 10 | 0.287 | 0.186 | 1.68/4.07 | 1.01 | 1.69 | 0.97 | 983 |
| nemotron_mlx:bf16-auto | real quiet (_q) | 10 | 0.309 | 0.215 | 0.93/2.11 | 1.24 | 0.14 | 0.14 | 1650 |
| voxtral_mlx:4bit-480ms | real quiet (_q) | 10 | 0.203 | 0.140 | 30.05/32.76 | 1.56 | 30.49 | 1.68 | 3583 |
| whisper_cpp:turbo-q5-ac768-v2 | real quiet (_q) | 10 | 0.233 | 0.152 | 2.17/4.20 | 0.85 | 1.51 | 0.96 | 996 |
| nemotron_mlx:bf16-auto | secondary (FLEURS/synthetic) | 4 | 0.108 | 0.052 | 0.76/1.44 | 1.34 | 0.14 | 0.14 | 1651 |
| voxtral_mlx:4bit-480ms | secondary (FLEURS/synthetic) | 4 | 0.046 | 0.013 | 29.80/33.79 | 1.44 | 30.67 | 1.60 | 3518 |
| whisper_cpp:turbo-q5-ac768-v2 | secondary (FLEURS/synthetic) | 4 | 0.159 | 0.108 | 2.15/4.01 | 1.65 | 1.35 | 0.93 | 994 |

### English terms: RU with EN jargon (01-04, real; quiet+noisy)

| engine | exact Latin | Cyrillic transliteration | other mismatch | missing | exact % |
|---|---|---|---|---|---|
| nemotron_mlx:bf16-auto | 46 | 143 | 27 | 0 | 21% |
| voxtral_mlx:4bit-480ms | 123 | 72 | 21 | 0 | 57% |
| whisper_cpp:turbo-q5-ac768-v2 | 117 | 66 | 33 | 0 | 54% |

### English terms: English-heavy with RU inserts (05, real)

| engine | exact Latin | Cyrillic transliteration | other mismatch | missing | exact % |
|---|---|---|---|---|---|
| nemotron_mlx:bf16-auto | 186 | 22 | 55 | 7 | 69% |
| voxtral_mlx:4bit-480ms | 242 | 8 | 18 | 2 | 90% |
| whisper_cpp:turbo-q5-ac768-v2 | 206 | 44 | 19 | 1 | 76% |

### Per-term rendering (01-04; ok = kept in Latin, otherwise what the engine wrote)

| term | nemotron_mlx:bf16-auto | voxtral_mlx:4bit-480ms | whisper_cpp:turbo-q5-ac768-v2 |
|---|---|---|---|
| rate | 0/6 -> reid; right batterry | 4/6 -> бернрейд; берн рейд | 3/6 -> ride; arr |
| user | 0/4 -> юзер; юн дизерс интервью с | 0/4 -> юзер; юндизерс интервью с | 2/4 -> users; юзер |
| metric | 2/4 -> примари метрик | 3/4 -> метрик | 3/4 -> northstarmetric |
| maps | 2/4 -> оффлайн мапс | 4/4 | 4/4 |
| month | 1/4 -> манфаромат мафлин; math | 3/4 -> aromat | 3/4 -> or amount матери берем 3 |
| burn | 0/4 -> манфаромат мафлин; берn | 2/4 -> бернрейд; берн рейд | 0/4 -> or amount матери берем 3; музыка музыка берн 3 |
| ltv | 3/4 -> лтв | 4/4 | 2/4 -> 8 лтв; юнит экономикс лтв |
| cac | 2/4 -> сас | 2/4 -> sas 240; saas | 2/4 -> sas 240; сас |
| latency | 0/4 -> лайтенсии; лайтенсии найдпоинт слэш | 3/4 -> лейпингом | 3/4 -> стоит принцы |
| sla | 2/4 -> слай; сла | 4/4 | 4/4 |
| fintech | 0/2 -> pintech startip; финтеч | 0/2 -> пентек; финтедж | 1/2 -> pintech |
| product | 0/2 -> продукт | 0/2 -> продукт дискавери | 2/2 |
| discovery | 2/2 | 0/2 -> продукт дискавери | 2/2 |
| interviews | 0/2 -> юн дизерс интервью с; юзер интервью с | 0/2 -> юндизерс интервью с; юзер интервью | 2/2 |
| prd | 1/2 -> пирди | 2/2 | 1/2 -> aprd |
| flow | 0/2 -> запускали новую флоу; флоу активышн райт вырас | 2/2 | 1/2 -> запускали новую флоу |
| activation | 1/2 -> флоу активышн райт вырас | 2/2 | 2/2 |
| retention | 1/2 -> рентеншон dirty | 2/2 | 1/2 -> рентеншн |
| d30 | 0/2 -> рентеншон dirty; до 30 | 1/2 -> до 30 | 1/2 -> до 30 |
| cohort | 0/2 -> кухортам; кохортам | 0/2 -> кухортам; кохардам | 0/2 -> кухортам; кохортам |
| ads | 0/2 -> и цас; адс | 1/2 -> ecs | 1/2 -> itsas |
| intent | 0/2 -> интент | 2/2 | 2/2 |
| north | 1/2 -> nothar | 1/2 -> node | 1/2 -> northstarmetric |
| star | 1/2 -> nothar | 2/2 | 1/2 -> northstarmetric |
| engaged | 2/2 | 2/2 | 2/2 |
| users | 2/2 | 2/2 | 2/2 |
| rice | 0/2 -> rais; райс | 0/2 -> rise | 0/2 -> райс а; rise |
| ice | 0/2 -> is; айс | 2/2 | 1/2 -> айс |
