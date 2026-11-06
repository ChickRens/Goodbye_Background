"""移植の正しさの検証（遅いテスト）: bench.py と同じ動画・同じ点で low-res マスクの IoU を比べる。

環境変数:
  BENCH_PY       bench.py（使い捨てスパイク）のパス。必須
  SAM2_MODEL_DIR fetch_models で取得済みのモデルディレクトリ（constants.json と onnx/）。必須
  BENCH_VIDEO    入力動画。無ければ ffmpeg でテスト用の短い動画を作る
実行: BENCH_PY=... SAM2_MODEL_DIR=... uv run pytest -m slow tests/test_tracker_parity.py -v
"""

import importlib.util
import os
import subprocess
import sys
from pathlib import Path

import cv2
import numpy as np
import pytest

from cutout.contract import NormalizedPoint
from cutout.models import GRAPH_NAMES, load_constants, load_sessions
from cutout.tracker import Sam2VideoTracker

FRAMES = 10
POINT = (0.51, 0.44)


def _require_env(name: str) -> str:
    v = os.environ.get(name)
    if not v:
        pytest.skip(f"{name} が未設定")
    return v


def _load_bench(path: Path):
    # bench.py は同じディレクトリの convert.py を import するので、そのディレクトリを探索パスに足す
    sys.path.insert(0, str(path.parent))
    try:
        spec = importlib.util.spec_from_file_location("bench_spike", path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        return mod
    except ImportError as e:
        pytest.skip(f"bench.py を import できない: {e}")
    finally:
        sys.path.remove(str(path.parent))


def _video(tmp_path: Path) -> str:
    v = os.environ.get("BENCH_VIDEO")
    if v:
        return v
    out = tmp_path / "clip.mp4"
    subprocess.run(
        ["ffmpeg", "-y", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=30", "-t", "1",
         "-pix_fmt", "yuv420p", str(out)],
        check=True, capture_output=True,
    )
    return str(out)


def _read_bgr_frames(path: str, n: int) -> list[np.ndarray]:
    cap = cv2.VideoCapture(path)
    frames = []
    while len(frames) < n:
        ok, f = cap.read()
        if not ok:
            break
        frames.append(f)
    cap.release()
    return frames


def _iou(a: np.ndarray, b: np.ndarray) -> float:
    a, b = a > 0, b > 0
    u = np.logical_or(a, b).sum()
    return 1.0 if u == 0 else float(np.logical_and(a, b).sum() / u)


@pytest.mark.slow
def test_low_res_masks_match_bench(tmp_path):
    bench_py = Path(_require_env("BENCH_PY"))
    model_dir = Path(_require_env("SAM2_MODEL_DIR"))
    if not bench_py.exists():
        pytest.skip(f"BENCH_PY が存在しない: {bench_py}")
    missing = [p for p in ["constants.json", *[f"onnx/{g}.onnx" for g in GRAPH_NAMES]]
               if not (model_dir / p).exists()]
    if missing:
        pytest.skip(f"SAM2_MODEL_DIR にモデルが無い: {missing}")

    bench = _load_bench(bench_py)
    frames = _read_bgr_frames(_video(tmp_path), FRAMES)
    assert len(frames) == FRAMES

    consts = load_constants(model_dir)
    tpos = np.array(consts["memory_temporal_positional_encoding"], np.float32)

    # 基準: bench.py の track（fp16 のまま CPU、全フレーム推論）
    graphs = {g: bench.Graph(model_dir / "onnx" / f"{g}.onnx", os.cpu_count()) for g in GRAPH_NAMES}
    pts = [(POINT[0] * bench.S, POINT[1] * bench.S, 1)]
    expected, _, _ = bench.track(
        graphs, frames, pts, tpos, consts["image_mean"], consts["image_std"], stride=1
    )

    # 検証対象: Sam2VideoTracker（入力は RGB なので BGR から変換する）
    tracker = Sam2VideoTracker(load_sessions(model_dir, "cpu"), consts, total_frames=FRAMES)
    for i, bgr in enumerate(frames):
        pts_in = [NormalizedPoint(POINT[0], POINT[1], "include")] if i == 0 else None
        tracker.process(cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB), pts_in)
        iou = _iou(tracker.last_low_res, expected[i])
        assert iou >= 0.99, f"フレーム {i}: IoU {iou:.4f}"
