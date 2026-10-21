import subprocess, struct, sys, json, wave, os, time, re, tempfile, glob
ROOT=os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
W=os.environ.get("LOWK_STT_WORKER",ROOT+"/workers/lowk-stt-worker/target/release/lowk-stt-worker")
M=os.environ.get("LOWK_MODELS",os.path.expanduser("~/Library/Application Support/Lowk/models"))+"/nemotron-local/"
def run(wav, extra, right="3"):
    w=wave.open(wav); pcm=w.readframes(w.getnframes())
    args=[W,"stream","--model",M+"nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf","--vad",M+"silero-vad-v6.2-F32.gguf","--lang","auto","--right",right]+extra
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
def oolc(s): return sum(1 for c in s if c.isalpha() and not (c.isascii() or 'Ѐ'<=c<='ӿ'))+sum(1 for c in s if c in 'іїєґІЇЄҐўЎ')
if __name__=="__main__":
    # S3 regression: no letters outside ru+en on the stt-bench recordings (needs macOS afconvert for m4a -> wav).
    tmp=tempfile.mkdtemp(); bad=0; cers=[]
    for f in sorted(glob.glob(ROOT+"/spikes/stt-bench/audio/0*.m4a")):
        n=os.path.basename(f)[:-4]; wav=tmp+"/"+n+".wav"
        subprocess.run(["afconvert","-f","WAVE","-d","LEI16@16000","-c","1",f,wav],check=True)
        t=" ".join(e["text"] for e in run(wav,["--allowed","ru,en"]))
        o=oolc(t); bad+=o; cers.append(cer(t,open(f[:-4]+".txt").read()))
        print(n,"cer=%.3f ool=%d"%(cers[-1],o),flush=True)
    print("mean CER %.4f, out-of-language letters %d"%(sum(cers)/len(cers),bad))
    sys.exit(1 if bad else 0)
