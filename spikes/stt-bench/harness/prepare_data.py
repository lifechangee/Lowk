"""Build PRELIMINARY public test clips into audio/.
  fleurs : concatenates FLEURS ru_ru test utterances (CC-BY 4.0) into ~60 s clips, 16 kHz mono WAV + .txt
  synth  : macOS `say` (Milena RU + Samantha EN) code-switched RU/EN script -> synthetic, NOT real speech
Usage: python harness/prepare_data.py fleurs <parquet> | synth
"""
import sys, io, subprocess, tempfile, pathlib
import numpy as np, soundfile as sf, soxr
AUDIO = pathlib.Path(__file__).resolve().parent.parent / "audio"

def fleurs(parquet, n_clips=3, per_clip=5):
    import pyarrow.parquet as pq
    rows = pq.read_table(parquet, columns=["audio", "raw_transcription", "gender", "id"]).to_pylist()
    # alternate genders inside a clip so it resembles a two-speaker exchange
    men = [r for r in rows if r["gender"] == 0]; women = [r for r in rows if r["gender"] == 1]
    for c in range(n_clips):
        picks = []
        for i in range(per_clip):
            pool = men if (i + c) % 2 == 0 else women
            picks.append(pool[c * per_clip + i])
        wav, txt = [], []
        for r in picks:
            x, sr = sf.read(io.BytesIO(r["audio"]["bytes"]), dtype="float32")
            if x.ndim > 1: x = x.mean(1)
            wav.append(soxr.resample(x, sr, 16000)); wav.append(np.zeros(int(0.6 * 16000), np.float32))
            txt.append(r["raw_transcription"].strip())
        name = f"fleurs_ru_{c+1}"
        sf.write(AUDIO / f"{name}.wav", np.concatenate(wav), 16000, subtype="PCM_16")
        (AUDIO / f"{name}.txt").write_text(" ".join(txt) + "\n")
        print(name, round(sum(len(w) for w in wav) / 16000, 1), "s")

SYNTH = [  # (voice, text)
 ("Milena", "Давайте посмотрим на"), ("Samantha", "EBITDA"), ("Milena", "за прошлый квартал, а потом обсудим"), ("Samantha", "KPI"),
 ("Milena", "для команды продаж."), ("Milena", "Нам нужно задеплоить новую версию до конца"), ("Samantha", "sprint"),
 ("Milena", "и закрыть все тикеты в"), ("Samantha", "Jira."), ("Milena", "Потом созвонимся с клиентом по поводу"), ("Samantha", "pricing and onboarding flow"),
 ("Milena", "и обновим"), ("Samantha", "roadmap"), ("Milena", "на следующий год.")]
def synth():
    parts, txt = [], []
    for v, t in SYNTH:
        with tempfile.TemporaryDirectory() as d:
            f = pathlib.Path(d) / "a.aiff"
            subprocess.run(["say", "-v", v, "-o", str(f), t], check=True)
            x, sr = sf.read(f, dtype="float32")
            if x.ndim > 1: x = x.mean(1)
            parts.append(soxr.resample(x, sr, 16000)); parts.append(np.zeros(int(0.08 * 16000), np.float32))
        txt.append(t)
    sf.write(AUDIO / "synth_codeswitch_ru_en.wav", np.concatenate(parts), 16000, subtype="PCM_16")
    (AUDIO / "synth_codeswitch_ru_en.txt").write_text(" ".join(txt) + "\n")
    print("synth ok")

if __name__ == "__main__":
    if sys.argv[1] == "fleurs": fleurs(sys.argv[2])
    else: synth()
