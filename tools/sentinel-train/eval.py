"""Evaluate a trained ONNX paste classifier against a holdout set.

Emits per-label and macro TPR/FPR/FNR; gates Phase 4 ratification.
"""

from __future__ import annotations

import argparse
import json
import sys

import numpy as np
import onnxruntime as ort


def evaluate(args) -> int:
    data = np.load(args.holdout, allow_pickle=True)
    X = data["X"].astype(np.float32)
    y = data["y"].astype(np.int64)
    label_idx = data["label_idx"]
    label_names = list(data["label_names"])

    session = ort.InferenceSession(args.model, providers=["CPUExecutionProvider"])
    in_name = session.get_inputs()[0].name
    scores = session.run(None, {in_name: X})[0].reshape(-1)
    preds = (scores >= args.threshold).astype(np.int64)

    per_label: dict[str, dict] = {}
    for idx, name in enumerate(label_names):
        mask = label_idx == idx
        if not mask.any():
            continue
        y_l = y[mask]
        p_l = preds[mask]
        pos = (y_l == 1)
        neg = (y_l == 0)
        tpr = (p_l[pos] == 1).mean() if pos.any() else None
        fpr = (p_l[neg] == 1).mean() if neg.any() else None
        per_label[name] = {
            "count": int(mask.sum()),
            "tpr": None if tpr is None else float(tpr),
            "fpr": None if fpr is None else float(fpr),
        }

    pos = y == 1
    neg = y == 0
    macro_tpr = float((preds[pos] == 1).mean()) if pos.any() else None
    macro_fpr = float((preds[neg] == 1).mean()) if neg.any() else None
    gate_ok = (
        macro_tpr is not None
        and macro_fpr is not None
        and macro_tpr >= 0.92
        and macro_fpr <= 0.03
        and (per_label.get("paste_macro", {}).get("tpr") or 1.0) >= 0.98
        and (per_label.get("llm_paste_edit", {}).get("tpr") or 1.0) >= 0.85
    )

    result = {
        "model": args.model,
        "threshold": args.threshold,
        "samples": int(len(y)),
        "macro_tpr": macro_tpr,
        "macro_fpr": macro_fpr,
        "per_label": per_label,
        "gate_ok": bool(gate_ok),
    }
    if args.out:
        with open(args.out, "w") as f:
            json.dump(result, f, indent=2)
        print(f"wrote {args.out}")
    print(json.dumps(result, indent=2))
    return 0 if gate_ok else 2


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--model", required=True)
    p.add_argument("--holdout", required=True)
    p.add_argument("--threshold", type=float, default=0.5)
    p.add_argument("--out", default=None)
    args = p.parse_args()
    return evaluate(args)


if __name__ == "__main__":
    sys.exit(main())
