"""Featurize Sentinel synthetic blobs into (X, y) arrays.

Mirrors src/utils/sentinel/paste-features.ts exactly. If you change the
feature order or the FEATURE_DIM constant, update both files together —
the TS extractor and this script must produce byte-identical features
for the same input or the trained ONNX will silently mispredict at
runtime.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from glob import glob

import numpy as np

FEATURE_DIM = 12

# Snapshot window in milliseconds. The TS extractor passes the actual
# elapsed window; for synthetic samples we use a 30s window so the char
# rate feature is on the same scale as live data.
WINDOW_MS = 30_000.0

# Attack labels that map to y=1; everything else maps to y=0 (negative class).
POSITIVE_LABELS = {
    "paste_macro",
    "typing_bot_constant",
    "typing_bot_jitter",
    "llm_paste_edit",
    "remote_control",
}


def windowed_features(sample: dict) -> np.ndarray:
    """Convert one keystroke sample to a 12-dim feature vector."""
    dwell = np.asarray(sample["dwell_ms"], dtype=np.float32)
    flight = np.asarray(sample["flight_ms"], dtype=np.float32)
    n = len(dwell)
    out = np.zeros(FEATURE_DIM, dtype=np.float32)
    if n == 0:
        return out

    dwell_pos = dwell[dwell > 0]
    flight_pos = flight[flight > 0]

    mean_d = float(dwell_pos.mean()) if dwell_pos.size else 0.0
    std_d = float(dwell_pos.std()) if dwell_pos.size else 0.0
    mean_f = float(flight_pos.mean()) if flight_pos.size else 0.0
    std_f = float(flight_pos.std()) if flight_pos.size else 0.0

    near_zero_mask = flight < 5.0
    near_zero_count = int(near_zero_mask.sum())
    max_run = 0
    cur = 0
    for is_near in near_zero_mask.tolist():
        if is_near:
            cur += 1
            if cur > max_run:
                max_run = cur
        else:
            cur = 0

    char_rate = (n / WINDOW_MS) * 1000.0 if WINDOW_MS > 0 else 0.0

    out[0] = mean_d
    out[1] = std_d
    out[2] = mean_f
    out[3] = std_f
    out[4] = near_zero_count / n if n else 0.0
    out[5] = min(max_run, 200) / 200.0
    out[6] = min(char_rate, 50.0) / 50.0
    out[7] = std_d / mean_d if mean_d > 0.01 else 0.0
    out[8] = std_f / mean_f if mean_f > 0.01 else 0.0
    out[9] = 0.0  # paste_event_count is 0 for synthetic — live data fills in
    out[10] = 0.0  # pasted_char_count same
    out[11] = min(n, 200) / 200.0

    out[~np.isfinite(out)] = 0.0
    return out


def load_blob(path: str) -> tuple[np.ndarray, np.ndarray, str]:
    with open(path) as f:
        blob = json.load(f)
    label = blob["label"]
    y_val = 1 if label in POSITIVE_LABELS else 0
    feats = []
    for sample in blob["samples"]:
        feats.append(windowed_features(sample))
    if not feats:
        return np.zeros((0, FEATURE_DIM), dtype=np.float32), np.zeros(0, dtype=np.int64), label
    X = np.stack(feats)
    y = np.full(len(feats), y_val, dtype=np.int64)
    return X, y, label


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--in", dest="input_dir", required=True, help="Directory of .json blobs")
    p.add_argument("--out", dest="output", required=True, help="Output .npz path")
    args = p.parse_args()

    blob_paths = sorted(glob(os.path.join(args.input_dir, "*.json")))
    if not blob_paths:
        print(f"no .json blobs in {args.input_dir}", file=sys.stderr)
        return 1

    X_all, y_all, label_index = [], [], []
    label_lookup: dict[str, int] = {}
    for path in blob_paths:
        X, y, label = load_blob(path)
        if X.shape[0] == 0:
            continue
        if label not in label_lookup:
            label_lookup[label] = len(label_lookup)
        idx = label_lookup[label]
        X_all.append(X)
        y_all.append(y)
        label_index.append(np.full(len(y), idx, dtype=np.int32))
        print(f"  {label}: {X.shape[0]} samples")

    X = np.concatenate(X_all, axis=0)
    y = np.concatenate(y_all, axis=0)
    label_idx = np.concatenate(label_index, axis=0)
    np.savez_compressed(
        args.output,
        X=X,
        y=y,
        label_idx=label_idx,
        label_names=np.asarray(
            [name for name, _ in sorted(label_lookup.items(), key=lambda kv: kv[1])]
        ),
    )
    print(f"wrote {X.shape[0]} samples ({X.shape[1]}-dim) → {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
