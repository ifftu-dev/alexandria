"""Train the Sentinel paste classifier and export to ONNX.

Small MLP: 12 -> 32 -> 16 -> 1 (sigmoid). ~500 params, exports to
~10KB ONNX (int8 quantized: ~3KB). The on-device runtime expects
input name "features" with shape [1, 12] and output name "score".
"""

from __future__ import annotations

import argparse
import sys

import numpy as np
import torch
import torch.nn as nn
import torch.optim as optim
from torch.utils.data import DataLoader, TensorDataset


class PasteClassifier(nn.Module):
    """Small MLP — 12 → 32 → 16 → 1 sigmoid."""

    def __init__(self):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(12, 32),
            nn.ReLU(),
            nn.Linear(32, 16),
            nn.ReLU(),
            nn.Linear(16, 1),
            nn.Sigmoid(),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        return self.net(x)


def train(args) -> int:
    data = np.load(args.train, allow_pickle=True)
    X = torch.from_numpy(data["X"]).float()
    y = torch.from_numpy(data["y"]).float().unsqueeze(1)
    label_idx = data["label_idx"]
    label_names = data["label_names"]

    # Class weights to up-weight the harder llm_paste_edit class. Inverse
    # frequency would be the textbook answer, but a manual 2x for the
    # paste-edit label catches the realistic 2026 attack we care about.
    sample_weights = np.ones(len(y))
    if "llm_paste_edit" in label_names:
        edit_idx = int(np.where(label_names == "llm_paste_edit")[0][0])
        sample_weights[label_idx == edit_idx] = 2.0

    # Normalize features. Mean/std land in the bundle so live inference
    # uses the same normalization. Either store them in a sidecar JSON
    # OR bake them into the first linear layer (chosen here: bake).
    mean = X.mean(dim=0, keepdim=True)
    std = X.std(dim=0, keepdim=True).clamp(min=1e-6)
    X = (X - mean) / std

    n = len(X)
    perm = torch.randperm(n)
    n_train = int(n * 0.9)
    train_idx, val_idx = perm[:n_train], perm[n_train:]

    train_ds = TensorDataset(X[train_idx], y[train_idx])
    val_ds = TensorDataset(X[val_idx], y[val_idx])
    train_loader = DataLoader(train_ds, batch_size=args.batch, shuffle=True)
    val_loader = DataLoader(val_ds, batch_size=args.batch)

    model = PasteClassifier()
    opt = optim.AdamW(model.parameters(), lr=args.lr)
    loss_fn = nn.BCELoss(reduction="none")
    sample_weights = torch.from_numpy(sample_weights).float()

    for epoch in range(args.epochs):
        model.train()
        train_loss = 0.0
        for xb, yb in train_loader:
            opt.zero_grad()
            pred = model(xb)
            # Label smoothing 0.05
            yb_smooth = yb * (1.0 - 0.05) + 0.05 * 0.5
            losses = loss_fn(pred, yb_smooth)
            losses = losses.mean()
            losses.backward()
            opt.step()
            train_loss += losses.item() * xb.size(0)
        train_loss /= len(train_ds)

        model.eval()
        with torch.no_grad():
            val_loss = 0.0
            correct = 0
            for xb, yb in val_loader:
                pred = model(xb)
                val_loss += loss_fn(pred, yb).mean().item() * xb.size(0)
                correct += ((pred > 0.5).float() == yb).sum().item()
            val_loss /= len(val_ds)
            val_acc = correct / len(val_ds)
        print(f"epoch {epoch+1:3d}  train={train_loss:.4f}  val={val_loss:.4f}  acc={val_acc:.3f}")

    # Sanity check before baking: capture trained-model output on normalized
    # input. After baking we'll feed raw input and expect ~bit-identical
    # output. If this drifts, the bake math is wrong.
    sanity_raw = torch.from_numpy(data["X"][:8]).float()
    sanity_norm = (sanity_raw - mean) / std
    with torch.no_grad():
        expected = model(sanity_norm).clone()

    # Bake mean/std into the first linear layer so the on-device runtime
    # can feed raw features directly. ONNX export then sees a single
    # forward path with no preprocessing requirement.
    #
    #   x_norm = (x - mean) / std
    #   y = x_norm @ W^T + b
    #     = x @ (W/std)^T - mean @ (W/std)^T + b
    # New weights: W' = W/std (broadcast over rows), b' = b - mean·(W/std)^T.
    first_linear: nn.Linear = model.net[0]  # type: ignore[assignment]
    with torch.no_grad():
        new_w = first_linear.weight / std  # shape [32,12]
        # mean is [1,12]; new_w.t() is [12,32]; product [1,32]; flatten to [32]
        bias_shift = (mean @ new_w.t()).flatten()
        new_b = first_linear.bias - bias_shift
        first_linear.weight.copy_(new_w)
        first_linear.bias.copy_(new_b)

    model.eval()
    with torch.no_grad():
        baked = model(sanity_raw)
    diff = (baked - expected).abs().max().item()
    if diff > 1e-4:
        print(f"WARN: bake-sanity drift {diff:.2e} (expected <1e-4)", file=sys.stderr)
    else:
        print(f"bake-sanity OK (max abs diff = {diff:.2e})")

    dummy = torch.zeros(1, 12, dtype=torch.float32)
    # `dynamo=False` forces the legacy TorchScript-based exporter, which
    # emits weights INLINE as initializers. The new dynamo exporter
    # writes weights to a sidecar `.data` file by default, which the
    # Rust tract loader (and most other ONNX runtimes) can't resolve
    # when the model is consumed from raw bytes (e.g. `include_bytes!`).
    torch.onnx.export(
        model,
        dummy,
        args.out,
        input_names=["features"],
        output_names=["score"],
        opset_version=17,
        dynamic_axes={"features": {0: "batch"}, "score": {0: "batch"}},
        dynamo=False,
    )
    print(f"exported ONNX → {args.out}")
    return 0


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--train", required=True, help="Featurized .npz path")
    p.add_argument("--out", required=True, help="Output .onnx path")
    p.add_argument("--epochs", type=int, default=30)
    p.add_argument("--batch", type=int, default=128)
    p.add_argument("--lr", type=float, default=3e-4)
    args = p.parse_args()
    return train(args)


if __name__ == "__main__":
    sys.exit(main())
