"""Re-export YuNet (face_detection_yunet_2023mar) with dynamic spatial input.

Usage (needs onnx + onnxruntime + numpy; a throwaway venv is fine):
  python scripts/sentinel/export-yunet-dynamic.py <face_detection_yunet_2023mar.onnx> <out.onnx>

The OpenCV Zoo export declares a fixed [1,3,640,640] input, the matching
[1,6400|1600|400,*] outputs and 94 intermediate value_info shapes, but every
op in the graph is already shape-agnostic: the twelve head Reshapes use -1
and both Resizes use scales. Dropping the baked shapes (weights untouched)
lets the detector run at the frame's own resolution, padded to a multiple of
32. Writes <out.onnx>.sha256 and prints the onnxruntime reference outputs the
parity test in src-tauri/src/sentinel/face_detect.rs pins.
"""
import hashlib
import json
import sys
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort

src = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()

model = onnx.load(str(src))
graph = model.graph
assert [d.dim_value for d in graph.input[0].type.tensor_type.shape.dim] == [1, 3, 640, 640]
dims = graph.input[0].type.tensor_type.shape.dim
for i, name in ((2, "height"), (3, "width")):
    dims[i].ClearField("dim_value")
    dims[i].dim_param = name
for output in graph.output:
    anchors = output.type.tensor_type.shape.dim[1]
    anchors.ClearField("dim_value")
    anchors.dim_param = "anchors"
del graph.value_info[:]
onnx.checker.check_model(model)
onnx.save(model, str(out))

# Reference outputs for the Rust parity test: a deterministic gradient at the
# shape a 224x168 webview frame pads to.
session = ort.InferenceSession(str(out))
h, w = 192, 224
yy, xx = np.meshgrid(np.arange(h, dtype=np.float32), np.arange(w, dtype=np.float32), indexing="ij")
frame = np.stack([xx, yy, (xx + yy) * 0.5])[None] % 255.0
outputs = session.run(None, {"input": frame.astype(np.float32)})
names = [o.name for o in session.get_outputs()]
reference = {n: [float(o.sum()), list(o.shape)] for n, o in zip(names, outputs)}

digest = hashlib.sha256(out.read_bytes()).hexdigest()
(out.parent / (out.name + ".sha256")).write_text(f"{digest}  {out.name}\n")
print(json.dumps({"bytes": out.stat().st_size, "sha256": digest, "reference_sums": reference}, indent=1))
