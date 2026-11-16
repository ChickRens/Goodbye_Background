"""進捗・完了通知の送信（HMAC-SHA256 署名付き）。

Rust サーバー側が同じ署名方式で検証する。署名対象は「{timestamp}.」に続けて
送信する本文のバイト列そのもの（送信前に一度だけ直列化したものを使う）。

- progress: 0.5 秒未満の間隔の呼び出しは間引く。失敗は握りつぶす。
- final: 初回を含めて最大 4 回送る（失敗後 1・2・4 秒待ってから再送）。例外は出さない。
"""

from __future__ import annotations

import hashlib
import hmac
import json
from collections.abc import Callable

import httpx

PROGRESS_INTERVAL_SEC = 0.5
FINAL_BACKOFF_SEC = (1, 2, 4)  # 初回失敗後の待ち時間。合計 4 回送る

PostFn = Callable[[str, bytes, dict[str, str]], int]


def sign(secret: bytes, timestamp: int, body: bytes) -> str:
    """HMAC-SHA256(secret, f"{timestamp}.".encode() + body) の小文字 hex を返す。"""
    message = f"{timestamp}.".encode() + body
    return hmac.new(secret, message, hashlib.sha256).hexdigest()


def httpx_post(url: str, body: bytes, headers: dict[str, str]) -> int:
    """本番用の POST。HTTP ステータスコードを返す。"""
    return httpx.post(url, content=body, headers=headers, timeout=10).status_code


def _is_success(status: int) -> bool:
    return 200 <= status < 300


class Notifier:
    def __init__(
        self,
        callback_url: str,
        secret: bytes,
        post: PostFn,
        clock: Callable[[], float],
        sleep: Callable[[float], None],
    ) -> None:
        self._url = callback_url
        self._secret = secret
        self._post = post
        self._clock = clock
        self._sleep = sleep
        self._last_sent: float | None = None  # 直近に送信を試みた progress の時刻

    def progress(self, event: dict) -> None:
        now = self._clock()
        if self._last_sent is not None and now - self._last_sent < PROGRESS_INTERVAL_SEC:
            return
        self._last_sent = now
        try:
            self._send(event)
        except Exception:
            # progress は失敗しても処理を止めない
            pass

    def final(self, event: dict) -> bool:
        delays: list[float | None] = [None, *FINAL_BACKOFF_SEC]
        for delay in delays:
            if delay is not None:
                self._sleep(delay)
            try:
                status = self._send(event)
            except Exception:
                continue
            if _is_success(status):
                return True
        return False

    def _send(self, event: dict) -> int:
        body = json.dumps(event, separators=(",", ":")).encode()
        timestamp = int(self._clock())
        headers = {
            "Content-Type": "application/json",
            "X-Signature-Timestamp": str(timestamp),
            "X-Signature": sign(self._secret, timestamp, body),
        }
        return self._post(self._url, body, headers)
