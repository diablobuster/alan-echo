"""Download Kokoro-82M (timestamped ONNX) and patch it for DirectML.
Usage: py -3.11 -m pip install huggingface_hub onnx && py -3.11 tools/read-aloud/prepare_model.py
Writes to %APPDATA%/ALAN Echo/models/kokoro/."""
import os, sys, shutil
from huggingface_hub import hf_hub_download
import onnx
from onnx import helper

REPO = "onnx-community/Kokoro-82M-v1.0-ONNX-timestamped"
VOICES = ["af_heart", "am_michael", "bf_emma"]
dst = os.path.join(os.environ["APPDATA"], "ALAN Echo", "models", "kokoro")
os.makedirs(os.path.join(dst, "voices"), exist_ok=True)

def get(name):
    return hf_hub_download(REPO, name)

def patch(src, out):
    # DirectML fails on grouped ConvTranspose with output_padding.
    # Equivalent rewrite: drop output_padding, shrink the end pad by the same amount.
    m = onnx.load(src)
    n_patched = 0
    for n in m.graph.node:
        if n.op_type == "ConvTranspose":
            a = {x.name: x for x in n.attribute}
            if "output_padding" in a:
                op = list(a["output_padding"].ints); pads = list(a["pads"].ints)
                k = len(op); pads = pads[:k] + [pads[k + i] - op[i] for i in range(k)]
                n.attribute.remove(a["output_padding"]); n.attribute.remove(a["pads"])
                n.attribute.append(helper.make_attribute("pads", pads))
                n_patched += 1
    onnx.save(m, out)
    print(f"patched {n_patched} ConvTranspose nodes -> {out}")

patch(get("onnx/model_quantized.onnx"), os.path.join(dst, "model_q8_dml.onnx"))
shutil.copy(get("onnx/model.onnx"), os.path.join(dst, "model_fp32.onnx"))
shutil.copy(get("tokenizer.json"), os.path.join(dst, "tokenizer.json"))
for v in VOICES:
    shutil.copy(get(f"voices/{v}.bin"), os.path.join(dst, "voices", f"{v}.bin"))
print("done:", dst)
