"""R2 の署名付き URL との入出力（pipeline.Transfer の本番実装。Modal に依存しない）。

- 署名付き URL は秘密と同じ扱い。ログ・例外のメッセージには HTTP ステータスと例外の型だけを出す
  （httpx の例外や raise_for_status のメッセージは URL を含むので、そのまま外に出さない）。
- 入力 URL は GET 用に署名されている（SigV4 はメソッドも署名する）ので HEAD は送らない。
"""

from __future__ import annotations

import logging
import re
from pathlib import Path

import httpx

from cutout.errors import InputTooLarge

logger = logging.getLogger(__name__)

_CONTENT_RANGE_TOTAL = re.compile(r"^bytes \d+-\d+/(\d+)$")


class TransferError(Exception):
    """送受信の失敗。メッセージに URL は含めない。"""


def _default_client() -> httpx.Client:
    return httpx.Client(timeout=httpx.Timeout(30.0, read=60.0, write=60.0), follow_redirects=False)


class HttpTransfer:
    def __init__(self, client: httpx.Client | None = None) -> None:
        self._client = client or _default_client()

    def head_size(self, url: str) -> int | None:
        """入力の全体サイズ。GET に Range: bytes=0-0 を付けて Content-Range から読む。分からなければ None。"""
        try:
            with self._client.stream("GET", url, headers={"Range": "bytes=0-0"}) as r:
                if r.status_code == 206:
                    m = _CONTENT_RANGE_TOTAL.match(r.headers.get("Content-Range", ""))
                    return int(m.group(1)) if m else None
                if r.status_code == 200:  # Range を無視して全体を返す相手。本文は読まずに閉じる
                    length = r.headers.get("Content-Length")
                    return int(length) if length and length.isdigit() else None
                logger.warning("入力のサイズを取得できない (HTTP %d)", r.status_code)
                return None
        except httpx.HTTPError as e:
            logger.warning("入力のサイズを取得できない (%s)", type(e).__name__)
            return None

    def download(self, url: str, dest: Path, max_bytes: int) -> None:
        """ストリームで dest に保存する。max_bytes を超えた時点で止めて InputTooLarge を投げる。"""
        try:
            with self._client.stream("GET", url) as r:
                if r.status_code != 200:
                    raise TransferError(f"ダウンロードに失敗 (HTTP {r.status_code})")
                length = r.headers.get("Content-Length")
                if length and length.isdigit() and int(length) > max_bytes:
                    raise InputTooLarge()
                written = 0
                with dest.open("wb") as f:
                    for chunk in r.iter_bytes():
                        written += len(chunk)
                        if written > max_bytes:
                            raise InputTooLarge()
                        f.write(chunk)
        except httpx.HTTPError as e:
            raise TransferError(f"ダウンロードに失敗 ({type(e).__name__})") from None

    def upload(self, url: str, src: Path, content_type: str) -> None:
        """src を PUT する。Content-Type は渡されたものをそのまま付ける（Content-Length は httpx がファイルから付ける）。"""
        try:
            with src.open("rb") as f:
                r = self._client.put(url, content=f, headers={"Content-Type": content_type})
        except httpx.HTTPError as e:
            raise TransferError(f"アップロードに失敗 ({type(e).__name__})") from None
        if not 200 <= r.status_code < 300:
            raise TransferError(f"アップロードに失敗 (HTTP {r.status_code})")
