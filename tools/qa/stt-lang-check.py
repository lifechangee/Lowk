import subprocess, struct, sys, json, wave, os, time, re, tempfile, glob, argparse, shutil
from collections import Counter
ROOT=os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
W=os.environ.get("LOWK_STT_WORKER",os.path.join(ROOT,"workers","lowk-stt-worker","target","release","lowk-stt-worker" + (".exe" if os.name == "nt" else "")))
def default_model_dir():
    if os.environ.get("LOWK_MODELS"):
        return os.path.join(os.environ["LOWK_MODELS"], "nemotron-local")
    if os.name == "nt":
        marker = os.path.join(os.environ.get("APPDATA", ""), "Lowk", "data-root.txt")
        if os.path.isfile(marker):
            with open(marker, encoding="utf-8-sig") as f:
                return os.path.join(f.read().strip(), "models", "nemotron-local")
        return os.path.join(os.environ.get("LOCALAPPDATA", ""), "Lowk", "models", "nemotron-local")
    return os.path.expanduser("~/Library/Application Support/Lowk/models/nemotron-local")
M=os.environ.get("LOWK_NEMOTRON_MODEL_DIR", default_model_dir())
TAGS={"ru":"ru-RU","en":"en-US","pl":"pl-PL","de":"de-DE","uk":"uk-UA","fr":"fr-FR","es":"es-ES"}
def decoder_lang(langs): return TAGS.get(langs[0],"auto") if len(langs)==1 else "auto"  # = langpolicy::decoder_lang
def run(wav, extra, right="3", lang="auto"):
    w=wave.open(wav); pcm=w.readframes(w.getnframes())
    args=[W,"stream","--model",os.path.join(M,"nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf"),"--vad",os.path.join(M,"silero-vad-v6.2-F32.gguf"),"--lang",lang,"--right",right]+extra
    p=subprocess.Popen(args,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
    evs=[]
    import threading
    def rd():
        for l in p.stdout:
            try: evs.append(json.loads(l))
            except: pass
    t=threading.Thread(target=rd); t.start()
    # wait ready
    while not any(e.get("ev")=="ready" for e in evs):
        if p.poll() is not None: raise RuntimeError("worker exited before ready")
        time.sleep(0.05)
    fr=640 # 20ms
    t_ns=1_000_000_000
    n=len(pcm)//2
    for i in range(0,n,320):
        chunk=pcm[i*2:(i+320)*2]
        pl=struct.pack("<Q",t_ns)+chunk
        p.stdin.write(struct.pack("<BI",1,len(pl))+pl); t_ns+=20_000_000
    # trailing silence 1s
    z=b"\0"*640
    for _ in range(60):
        pl=struct.pack("<Q",t_ns)+z; p.stdin.write(struct.pack("<BI",1,len(pl))+pl); t_ns+=20_000_000
    j=json.dumps({"op":"flush"}).encode(); p.stdin.write(struct.pack("<BI",2,len(j))+j)
    j=json.dumps({"op":"stop"}).encode(); p.stdin.write(struct.pack("<BI",2,len(j))+j); p.stdin.flush()
    t.join(); p.wait()
    if p.returncode: raise RuntimeError("worker failed")
    return [e for e in evs if e.get("ev")=="stt" and e["kind"]=="final"]
def norm(s): return re.sub(r"[^\w ]","",s.lower().replace("ё","е")).split()
def cer(h,r):
    h=" ".join(norm(h)); r=" ".join(norm(r))
    d=list(range(len(r)+1))
    for i,a in enumerate(h,1):
        p=d[:]; d[0]=i
        for j,b in enumerate(r,1): d[j]=min(p[j]+1,d[j-1]+1,p[j-1]+(a!=b))
    return d[len(r)]/max(1,len(r))
def oolc(s, langs=("ru","en")):
    """Letters the call's languages cannot contain (mirror of langpolicy::is_foreign_letter for ru/en/uk/pl/de)."""
    cyr=any(l in ("ru","uk","bg") for l in langs); ukl=any(l in ("uk","be") for l in langs)
    lat={"pl":"ąćęłńóśźż","de":"äöüß","fr":"àâæçéèêëîïôœùûüÿ","es":"áéíóúüñ"}
    ok=set("".join(lat.get(l,"") for l in langs))
    n=0
    for c in s:
        if not c.isalpha() or c.isascii(): continue
        if c in "іїєґІЇЄҐўЎ": n+=0 if ukl else 1
        elif 'Ѐ'<=c<='ӿ': n+=0 if cyr else 1
        elif c.lower() in ok: pass
        else: n+=1
    return n
if __name__=="__main__":
    parser=argparse.ArgumentParser(description="Replay STT fixtures and measure language leakage.")
    parser.add_argument("sets", nargs="*", help="legacy language sets, e.g. ru,en")
    parser.add_argument("--quick", action="store_true", help="use the legacy quick fixture subset")
    parser.add_argument("--rates", action="store_true", help="report counts from live finals only")
    parser.add_argument("--langs", default="ru,en", help="comma-separated call languages for --rates")
    parser.add_argument("--dir", default=os.path.join(ROOT,"fixtures","stt"), help="directory of audio files for --rates")
    parser.add_argument("--worker", default=W, help="worker executable (or LOWK_STT_WORKER)")
    parser.add_argument("--model-dir", default=M, help="nemotron-local model directory (or LOWK_NEMOTRON_MODEL_DIR)")
    opts=parser.parse_args()
    W=opts.worker; M=opts.model_dir
    if opts.rates:
        langs=[x.strip().lower() for x in opts.langs.split(",") if x.strip()]
        if not langs: parser.error("--langs must contain at least one language")
        extensions={".wav", ".m4a", ".mp3", ".flac", ".ogg", ".opus"}
        clips={}
        entries=sorted(os.listdir(opts.dir)) if os.path.isdir(opts.dir) else []
        for f in (os.path.join(opts.dir, entry) for entry in entries):
            name, ext=os.path.splitext(os.path.basename(f))
            if os.path.isfile(f) and ext.lower() in extensions:
                if name not in clips or (ext.lower()==".wav" and os.path.splitext(clips[name])[1].lower()!=".wav"):
                    clips[name]=f
        if not clips:
            print("error: no audio clips found in " + opts.dir, file=sys.stderr)
            sys.exit(2)
        def ready_wav(path):
            if os.path.splitext(path)[1].lower()!=".wav": return False
            try:
                with wave.open(path, "rb") as audio:
                    return (audio.getframerate()==16000 and audio.getnchannels()==1
                            and audio.getsampwidth()==2 and audio.getcomptype()=="NONE")
            except (wave.Error, EOFError):
                return False
        converter=shutil.which("afconvert") or shutil.which("ffmpeg")
        if any(not ready_wav(f) for f in clips.values()) and not converter:
            print("error: converting audio requires afconvert or ffmpeg", file=sys.stderr)
            sys.exit(2)
        totals=[0,0,0,0]; total_leaks=Counter(); cers=[]
        print("clip segments outside no_lang foreign_letters cer leaks...", flush=True)
        with tempfile.TemporaryDirectory() as tmp:
            for i,(name,f) in enumerate(sorted(clips.items())):
                wav=f
                if not ready_wav(f):
                    wav=os.path.join(tmp, str(i)+".wav")
                    if os.path.basename(converter).lower().startswith("afconvert"):
                        cmd=[converter,"-f","WAVE","-d","LEI16@16000","-c","1",f,wav]
                    else:
                        cmd=[converter,"-nostdin","-loglevel","error","-y","-i",f,"-ac","1","-ar","16000","-sample_fmt","s16",wav]
                    subprocess.run(cmd,check=True)
                events=run(wav,["--allowed",",".join(langs)],lang=decoder_lang(langs))
                leaks=Counter(e["lang"] for e in events if e.get("lang") and e["lang"] not in langs)
                counts=[len(events),sum(leaks.values()),sum(not e.get("lang") for e in events),sum(oolc(e.get("text", ""),langs)>0 for e in events)]
                totals=[a+b for a,b in zip(totals,counts)]; total_leaks.update(leaks)
                ref_path=os.path.splitext(f)[0]+".txt"
                clip_cer="-"
                if os.path.isfile(ref_path):
                    with open(ref_path, encoding="utf-8") as ref:
                        value=cer(" ".join(e.get("text", "") for e in events), ref.read())
                    cers.append(value); clip_cer=f"{value:.3f}"
                print(name, *counts, clip_cer, *(f"{k}={v}" for k,v in sorted(leaks.items())), flush=True)
        mean_cer=f"{sum(cers)/len(cers):.3f}" if cers else "-"
        print("TOTAL", *totals, mean_cer, *(f"{k}={v}" for k,v in sorted(total_leaks.items())), flush=True)
        sys.exit(0)
    # S3 regression + per-call language sets: replays the stt-bench recordings (files only, no sound) per language set.
    # usage: stt-lang-check.py [ru,en] [ru] [en] ...   (default: ru,en); exit 1 on any foreign letter for a set.
    args=opts.sets
    quick=opts.quick  # 02_them_q (ru) + 03_me_q (en?) style subset used by qa-bundle
    sets=[x.split(",") for x in (args or ["ru,en"])]
    tmp=tempfile.mkdtemp(); fails=0; wavs=[]
    files=sorted(glob.glob(ROOT+"/fixtures/stt/0*.m4a"))
    if quick: files=[f for f in files if os.path.basename(f)[:-4] in ("02_them_q","04_me_q","01_me_n")]
    for f in files:
        n=os.path.basename(f)[:-4]; wav=tmp+"/"+n+".wav"
        subprocess.run(["afconvert","-f","WAVE","-d","LEI16@16000","-c","1",f,wav],check=True)
        wavs.append((n,wav,open(f[:-4]+".txt").read()))
    for langs in sets:
        bad=0; cers=[]; by={"ru":[],"en":[]}
        for n,wav,ref in wavs:
            t=" ".join(e["text"] for e in run(wav,["--allowed",",".join(langs)],lang=decoder_lang(langs)))
            o=oolc(t,langs); bad+=o; cers.append(cer(t,ref))
            by["ru" if re.search("[а-яА-Я]",ref) else "en"].append(cers[-1])
            print("%-6s %s cer=%.3f ool=%d"%("+".join(langs),n,cers[-1],o),flush=True)
        m=lambda v: sum(v)/len(v) if v else float("nan")
        print("SET %-6s mean CER %.4f (ru clips %.4f, en clips %.4f), letters outside the set: %d"%("+".join(langs),m(cers),m(by["ru"]),m(by["en"]),bad))
        fails+=bad
    sys.exit(1 if fails else 0)
