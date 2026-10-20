#!/usr/bin/env python
"""Lowk LLM latency bench via OpenRouter (streaming SSE). Key is read from env OPENROUTER_API_KEY only.
  OPENROUTER_API_KEY=... python bench.py [--runs 8] [--out results.json]
Measures TTFT (first non-empty *content* delta; reasoning deltas excluded), tokens/s over the content
phase, total time, usage and cost reported by OpenRouter. Each run gets a unique nonce in the user message
so provider prefix caches do not hide prefill (= worst-case first ask)."""
import argparse, base64, io, json, os, random, statistics, sys, time, uuid
import httpx

URL = "https://openrouter.ai/api/v1/chat/completions"
# (label, slug, extra request fields)
CONFIGS = [
    ("deepseek-v4.1-flash", "deepseek/deepseek-v4.1-flash", {}),
    ("deepseek-v4.1-flash sort=latency", "deepseek/deepseek-v4.1-flash", {"provider": {"sort": "latency"}}),
    ("deepseek-v4.1-flash:nitro", "deepseek/deepseek-v4.1-flash:nitro", {}),
    ("deepseek-v4.1-flash-1017 (1st party)", "deepseek/deepseek-v4.1-flash-1017", {}),
    ("claude-haiku-5.5", "anthropic/claude-haiku-5.5", {}),
    ("gemini-3.8-flash (reasoning=low, mandatory)", "google/gemini-3.8-flash", {"reasoning": {"effort": "low"}, "max_tokens": 1500}),
    ("gpt-6-luna", "openai/gpt-6-luna", {}),
    ("qwen3.8-flash", "qwen/qwen3.8-flash", {}),
    ("glm-5.3-flash (reasoning=low, mandatory)", "z-ai/glm-5.3-flash", {"reasoning": {"effort": "low"}, "max_tokens": 1500}),
]
SYSTEM = ("Ты — Lowk, тихий ассистент на встречах. Отвечай на языке вопроса пользователя, коротко (максимум 2-3 предложения), "
          "по делу, без вступлений и markdown-заголовков. Английские термины оставляй в латинице. Опирайся на транскрипт встречи "
          "и, если приложен скриншот, на экран собеседника.")
LINES = [
 "Ну смотри, по Q3 у нас выручка выросла до четырёх целых двух миллионов, но burn rate тоже подрос, потому что наняли двух backend-разработчиков.",
 "Согласен, но давай посмотрим на unit economics: CAC сейчас двести сорок долларов, а LTV по когорте июня около тысячи ста.",
 "Это значит LTV к CAC примерно четыре и шесть, что нормально для B2B SaaS, но payback period у нас девять месяцев, и это многовато.",
 "Инвесторы будут смотреть на EBITDA margin, а не только на growth, поэтому надо показать путь к безубыточности.",
 "Окей, а что с churn? В отчёте по retention у нас D30 держится на тридцати восьми процентах, а по enterprise-сегменту churn всего два процента в месяц.",
 "Да, enterprise тянет метрики вверх. Предлагаю сфокусировать roadmap на SSO, audit log и SLA на девяносто девять и девять.",
 "Подожди, а latency у нас на p95 сейчас какая? Клиенты из Европы жаловались, что дашборд грузится дольше трёх секунд.",
 "На p95 около двух и восьми, проблема в cold start у lambda-функций и в том, что мы не кэшируем агрегаты в Redis.",
 "Хорошо, заводим тикет, ставим приоритет P1, ответственный — Алексей. Дедлайн — конец спринта, обсудим на daily.",
 "Теперь про pricing. Мы думаем перейти с per-seat на usage-based, но тогда MRR станет менее предсказуемым, и финансовый директор против.",
 "Компромисс — hybrid: базовый платёж за seat плюс overage за API calls. Так сохраняем ARR visibility и даём гибкость.",
 "Ещё момент: pipeline на следующий квартал. Sales говорит, что в воронке два миллиона, но win rate у нас всего двадцать два процента.",
 "Значит ожидаемый bookings около четырёхсот сорока тысяч, и это ниже плана. Нужно либо поднимать conversion на этапе demo, либо расширять top of funnel.",
 "Маркетинг предлагает запустить ABM-кампанию на сорок целевых аккаунтов и вложить в неё тридцать тысяч долларов, ROI посчитаем через quarter.",
 "По продукту: A/B тест онбординга показал плюс одиннадцать процентов к activation, но sample size маленький, p-value ноль восемь, статистически не значимо.",
 "Давайте ещё две недели подержим эксперимент и вернёмся к решению, пока не раскатываем на всех пользователей.",
 "Последнее — hiring. Нужен staff engineer и один product manager с опытом fintech, бюджет в рамках headcount plan, который утвердили в сентябре.",
]
def transcript(n_chars=9500):
    rnd = random.Random(7); out = []; t = 0; who = ["Я", "Собеседник", "Собеседник 2"]; size = 0; i = 0
    while size < n_chars:
        l = LINES[i % len(LINES)]; i += 1; t += rnd.randint(9, 40)
        s = f"[{t//60:02d}:{t%60:02d}] {who[rnd.randint(0,2)]}: {l}"; out.append(s); size += len(s) + 1
    return "\n".join(out)
QUESTION = "А что значит EBITDA?"

def slide_png_b64():
    from PIL import Image, ImageDraw, ImageFont
    im = Image.new("RGB", (1280, 720), "white"); d = ImageDraw.Draw(im)
    def f(sz):
        for p in ("/System/Library/Fonts/Helvetica.ttc", "/System/Library/Fonts/Supplemental/Arial.ttf"):
            try: return ImageFont.truetype(p, sz)
            except Exception: pass
        return ImageFont.load_default()
    d.rectangle([0, 0, 1280, 110], fill=(20, 40, 90)); d.text((50, 30), "Q3 Financial Summary", font=f(52), fill="white")
    rows = [("Revenue", "$4.2M", "+18% QoQ"), ("Gross margin", "71%", "+2 pp"), ("EBITDA", "$0.6M", "margin 14%"), ("CAC", "$240", "LTV $1,100"), ("Burn rate", "$310k / mo", "runway 19 mo")]
    for k, (a, b, c) in enumerate(rows):
        y = 170 + k * 100; d.text((60, y), a, font=f(44), fill=(30, 30, 30)); d.text((520, y), b, font=f(44), fill=(0, 90, 160)); d.text((900, y), c, font=f(34), fill=(90, 90, 90))
    buf = io.BytesIO(); im.save(buf, "PNG"); return base64.b64encode(buf.getvalue()).decode()

def one(key, slug, extra, image_b64, tr):
    nonce = uuid.uuid4().hex[:8]
    text = f"(session {nonce})\nНовое в транскрипте:\n{tr}\n\nВопрос: {QUESTION}"
    content = [{"type": "text", "text": text}]
    if image_b64: content.append({"type": "image_url", "image_url": {"url": "data:image/png;base64," + image_b64}})
    body = dict(model=slug, stream=True, max_tokens=400, temperature=0.2, reasoning={"enabled": False},
                messages=[{"role": "system", "content": SYSTEM}, {"role": "user", "content": content}], usage={"include": True})
    body.update(extra)
    r = dict(ttft=None, ttft_any=None, total=None, tps=None, text="", provider=None, usage=None, error=None)
    t0 = time.perf_counter(); tl = None
    try:
        with httpx.Client(timeout=90) as c, c.stream("POST", URL, json=body, headers={"Authorization": "Bearer " + key, "X-Title": "lowk-bench"}) as resp:
            if resp.status_code != 200:
                r["error"] = f"HTTP {resp.status_code}: {resp.read().decode()[:300]}".replace(key, "<key>"); return r
            for line in resp.iter_lines():
                if not line.startswith("data: ") or line.endswith("[DONE]"): continue
                try: j = json.loads(line[6:])
                except Exception: continue
                now = time.perf_counter()
                if j.get("error"): r["error"] = str(j["error"])[:300]; break
                r["provider"] = j.get("provider") or r["provider"]
                if j.get("usage"): r["usage"] = j["usage"]
                for ch in j.get("choices", []):
                    dl = ch.get("delta", {})
                    if r["ttft_any"] is None and (dl.get("content") or dl.get("reasoning") or dl.get("reasoning_content")): r["ttft_any"] = now - t0
                    if dl.get("content"):
                        if r["ttft"] is None: r["ttft"] = now - t0
                        r["text"] += dl["content"]; tl = now
        r["total"] = time.perf_counter() - t0
        ct = (r["usage"] or {}).get("completion_tokens")
        if r["ttft"] is not None and tl and ct and tl - (r["ttft"] + t0) > 0.05: r["tps"] = (ct - 1) / (tl - t0 - r["ttft"])
    except Exception as e:
        r["error"] = f"{type(e).__name__}: {e}".replace(key, "<key>")[:300]
    return r

def pct(v, p):
    v = sorted(v); k = (len(v) - 1) * p / 100; lo = int(k); hi = min(lo + 1, len(v) - 1); return v[lo] + (v[hi] - v[lo]) * (k - lo)

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--runs", type=int, default=8); ap.add_argument("--out", default="results.json")
    ap.add_argument("--only", default=""); a = ap.parse_args()
    key = os.environ["OPENROUTER_API_KEY"]; tr = transcript(); img = slide_png_b64()
    cfgs = [c for c in CONFIGS if not a.only or a.only in c[0]]
    res = {}
    for rnd in range(a.runs):                      # interleave models per round to spread time-of-day noise
        for variant, im in (("text", None), ("text+image", img)):
            for label, slug, extra in cfgs:
                r = one(key, slug, extra, im, tr); res.setdefault(f"{label}|{variant}", []).append(r)
                print(f"r{rnd} {label:38s} {variant:10s} ttft={r['ttft'] and round(r['ttft'],2)} tps={r['tps'] and round(r['tps'])} prov={r['provider']} err={r['error']}", flush=True)
        json.dump(res, open(a.out, "w"), ensure_ascii=False, indent=1)
if __name__ == "__main__": main()
