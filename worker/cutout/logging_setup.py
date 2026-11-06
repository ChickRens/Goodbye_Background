"""ワーカーのログの設定。

httpx / httpcore は INFO で「HTTP Request: GET <URL 全体>」を出すので、署名付き URL がログに残る。
この 2 つは WARNING 以上だけにする（それ以上のレベルのメッセージにも URL を含めないのは transfer.py の責任）。
"""

import logging

_QUIET = ("httpx", "httpcore")


def configure_logging() -> None:
    logging.basicConfig(format="%(levelname)s %(name)s: %(message)s")  # 既にハンドラがあれば何もしない
    logging.getLogger().setLevel(logging.INFO)
    for name in _QUIET:
        logging.getLogger(name).setLevel(logging.WARNING)
