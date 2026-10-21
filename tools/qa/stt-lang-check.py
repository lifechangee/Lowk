import subprocess, struct, sys, json, wave, os, time, re, tempfile, glob
ROOT=os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
W=os.environ.get("LOWK_STT_WORKER",ROOT+"/workers/lowk-stt-worker/target/release/lowk-stt-worker")
M=os.environ.get("LOWK_MODELS",os.path.expanduser("~/Library/Application Support/Lowk/models"))+"/nemotron-local/"
TAGS={"ru":"ru-RU","en":"en-US","pl":"pl-PL","de":"de-DE","uk":"uk-UA","fr":"fr-FR","es":"es-ES"}
def decoder_lang(langs): return TAGS.get(langs[0],"auto") if len(langs)==1 else "auto"  # = langpolicy::decoder_lang
def run(wav, extra, right="3", lang="auto"):
    w=wave.open(wav); pcm=w.readframes(w.getnframes())
    args=[W,"stream","--model",M+"nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf","--vad",M+"silero-vad-v6.2-F32.gguf","--lang",lang,"--right",right]+extra
    p=subprocess.Popen(args,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
    evs=[]
    import threading
    def rd():
        for l in p.stdout:
            try: evs.append(json.loads(l))
            except: pass
    t=threading.Thread(target=rd); t.start()
    # wait ready
    while not any(e.get("ev")=="ready" for e in evs): time.sleep(0.05)
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
    # S3 regression + per-call language sets: replays the stt-bench recordings (files only, no sound) per language set.
    # usage: stt-lang-check.py [ru,en] [ru] [en] ...   (default: ru,en); exit 1 on any foreign letter for a set.
    args=[a for a in sys.argv[1:] if a!="--quick"]
    quick="--quick" in sys.argv  # 02_them_q (ru) + 03_me_q (en?) style subset used by qa-bundle
    sets=[x.split(",") for x in (args or ["ru,en"])]
    tmp=tempfile.mkdtemp(); fails=0; wavs=[]
    files=sorted(glob.glob(ROOT+"/spikes/stt-bench/audio/0*.m4a"))
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
