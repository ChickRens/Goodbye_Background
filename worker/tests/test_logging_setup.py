"""configure_logging の後、INFO でも署名付き URL がログに出ないことのテスト。"""

import logging

import httpx
import pytest

from cutout.logging_setup import configure_logging
from cutout.transfer import HttpTransfer

URL = "https://acct.r2.cloudflarestorage.com/bucket/video-jobs/j/input?X-Amz-Signature=secret-sig"


@pytest.fixture
def restore_logging():
    root = logging.getLogger()
    saved = (root.level, list(root.handlers), {n: logging.getLogger(n).level for n in ("httpx", "httpcore")})
    yield
    root.setLevel(saved[0])
    root.handlers[:] = saved[1]
    for n, lv in saved[2].items():
        logging.getLogger(n).setLevel(lv)


def test_presigned_urls_are_not_logged_at_info(tmp_path, caplog, restore_logging):
    configure_logging()

    def handler(r):
        if r.method == "PUT":
            return httpx.Response(200)
        if "Range" in r.headers:
            return httpx.Response(206, headers={"Content-Range": "bytes 0-0/3"}, content=b"a")
        return httpx.Response(200, content=b"abc")

    src = tmp_path / "out.mp4"
    src.write_bytes(b"m")
    t = HttpTransfer(httpx.Client(transport=httpx.MockTransport(handler)))
    with caplog.at_level(logging.INFO):
        assert t.head_size(URL) == 3
        t.download(URL, tmp_path / "input", max_bytes=10)
        t.upload(URL, src, "video/mp4")
    assert "secret-sig" not in caplog.text


def test_configure_logging_quiets_http_libraries(restore_logging):
    logging.getLogger("httpx").setLevel(logging.NOTSET)
    configure_logging()
    assert logging.getLogger().isEnabledFor(logging.INFO)
    for name in ("httpx", "httpcore"):
        assert not logging.getLogger(name).isEnabledFor(logging.INFO)
