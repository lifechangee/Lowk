"""Download the models used by the diarization spike into a scratch dir (never into the repo).
Usage: python fetch_models.py <dest_dir>
Prints URL + SHA-256 + size of every file (copy into docs/roadmap/diarization.md)."""
import hashlib, os, sys, urllib.request, tarfile

dest = sys.argv[1]
os.makedirs(dest, exist_ok=True)
GH = "https://github.com/k2-fsa/sherpa-onnx/releases/download/"
files = [
    GH + "speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
    GH + "speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
    GH + "speaker-recongition-models/nemo_en_titanet_small.onnx",
    GH + "speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx",
]
for u in files:
    p = os.path.join(dest, u.rsplit("/", 1)[1])
    if not os.path.exists(p):
        urllib.request.urlretrieve(u, p)
    if p.endswith(".tar.bz2"):
        tarfile.open(p).extractall(dest, filter="data")
    print(u, hashlib.sha256(open(p, "rb").read()).hexdigest(), os.path.getsize(p))

from huggingface_hub import snapshot_download
p = snapshot_download("nvidia/Nemotron-3-Diarization", local_dir=os.path.join(dest, "nemotron3"))
for f in sorted(os.listdir(p)):
    fp = os.path.join(p, f)
    if os.path.isfile(fp):
        print("hf:nvidia/Nemotron-3-Diarization/" + f, hashlib.sha256(open(fp, "rb").read()).hexdigest(), os.path.getsize(fp))
