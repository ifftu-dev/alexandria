"""Export Silent-Face-Anti-Spoofing's 2.7_80x80_MiniFASNetV2.pth to ONNX.

Usage (needs torch + onnx; a throwaway venv is fine):
  python scripts/sentinel/export-minifasnet.py <Silent-Face-Anti-Spoofing checkout> <out.onnx>
Then `shasum -a 256 <out.onnx> > <out.onnx>.sha256` next to the artifact and paste the
printed zeros/mid softmax into the parity test in src-tauri/src/sentinel/liveness.rs.

The exported graph takes a [1,3,80,80] float tensor in BGR channel order with
values in 0..255 (the repo's ToTensor does NOT divide by 255) and returns the
softmax over the 3 classes; index 1 is "real". Softmax is folded into the graph
so the Rust side reads probabilities directly.
"""
import hashlib
import json
import sys
from collections import OrderedDict
from pathlib import Path

import torch
import torch.nn.functional as F

repo = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
sys.path.insert(0, str(repo))

from src.model_lib.MiniFASNet import MiniFASNetV2  # noqa: E402
from src.utility import get_kernel  # noqa: E402

weights = repo / "resources/anti_spoof_models/2.7_80x80_MiniFASNetV2.pth"
model = MiniFASNetV2(conv6_kernel=get_kernel(80, 80))
state = torch.load(weights, map_location="cpu")
first = next(iter(state))
if first.startswith("module."):
    state = OrderedDict((k[7:], v) for k, v in state.items())
model.load_state_dict(state)
model.eval()


class WithSoftmax(torch.nn.Module):
    def __init__(self, inner):
        super().__init__()
        self.inner = inner

    def forward(self, x):
        return F.softmax(self.inner(x), dim=1)


wrapped = WithSoftmax(model).eval()
dummy = torch.zeros(1, 3, 80, 80)
torch.onnx.export(
    wrapped,
    dummy,
    str(out),
    input_names=["input"],
    output_names=["probs"],
    opset_version=11,
    do_constant_folding=True,
    dynamo=False,
)

# Reference outputs for the Rust parity test.
with torch.no_grad():
    zeros = wrapped(torch.zeros(1, 3, 80, 80))[0].tolist()
    mid = wrapped(torch.full((1, 3, 80, 80), 128.0))[0].tolist()
    # Deterministic pseudo-image: a smooth gradient so conv paths are exercised.
    yy, xx = torch.meshgrid(torch.arange(80.0), torch.arange(80.0), indexing="ij")
    grad = torch.stack([xx * 3.0, yy * 3.0, (xx + yy) * 1.5]).unsqueeze(0)
    gradient = wrapped(grad)[0].tolist()

digest = hashlib.sha256(out.read_bytes()).hexdigest()
(out.parent / (out.name + ".sha256")).write_text(f"{digest}  {out.name}\n")
print(json.dumps({"bytes": out.stat().st_size, "sha256": digest, "zeros": zeros, "mid": mid, "gradient": gradient}, indent=1))
