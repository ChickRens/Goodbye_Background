"""HttpTransfer のテスト。httpx.MockTransport で R2 の署名付き URL の振る舞いを再現する。"""

import httpx
import pytest

from cutout.errors import InputTooLarge
from cutout.transfer import HttpTransfer, TransferError

URL = "https://acct.r2.cloudflarestorage.com/bucket/video-jobs/j/input?X-Amz-Signature=secret-sig"


def transfer(handler) -> tuple[HttpTransfer, list[httpx.Request]]:
    seen: list[httpx.Request] = []

    def record(request: httpx.Request) -> httpx.Response:
        request.read()
        seen.append(request)
        return handler(request)

    return HttpTransfer(httpx.Client(transport=httpx.MockTransport(record))), seen


def test_head_size_uses_ranged_get_not_head():
    t, seen = transfer(lambda r: httpx.Response(206, headers={"Content-Range": "bytes 0-0/12345"}, content=b"x"))
    assert t.head_size(URL) == 12345
    assert seen[0].method == "GET" and seen[0].headers["Range"] == "bytes=0-0"


def test_head_size_full_response_uses_content_length():
    t, _ = transfer(lambda r: httpx.Response(200, content=b"abcdef"))
    assert t.head_size(URL) == 6


@pytest.mark.parametrize("status", [403, 416, 500])
def test_head_size_returns_none_on_error_status(status):
    t, _ = transfer(lambda r: httpx.Response(status))
    assert t.head_size(URL) is None


def test_head_size_returns_none_on_unknown_total():
    t, _ = transfer(lambda r: httpx.Response(206, headers={"Content-Range": "bytes 0-0/*"}, content=b"x"))
    assert t.head_size(URL) is None


def test_head_size_returns_none_on_network_error():
    def fail(r):
        raise httpx.ConnectError("boom", request=r)

    t, _ = transfer(fail)
    assert t.head_size(URL) is None


def test_download_writes_file(tmp_path):
    t, seen = transfer(lambda r: httpx.Response(200, content=b"0123456789"))
    dest = tmp_path / "input"
    t.download(URL, dest, max_bytes=10)
    assert dest.read_bytes() == b"0123456789" and seen[0].method == "GET"


def test_download_stops_when_stream_exceeds_max_bytes(tmp_path):
    read = []

    def chunks():
        for _ in range(100):
            read.append(1)
            yield b"x" * 10

    # Content-Length が無い（チャンク転送）ので、読みながら止める必要がある
    t, _ = transfer(lambda r: httpx.Response(200, content=chunks()))
    with pytest.raises(InputTooLarge):
        t.download(URL, tmp_path / "input", max_bytes=25)
    assert len(read) < 100


def test_download_rejects_large_content_length_before_reading(tmp_path):
    t, _ = transfer(lambda r: httpx.Response(200, headers={"Content-Length": "1000"}, content=b"x" * 1000))
    with pytest.raises(InputTooLarge):
        t.download(URL, tmp_path / "input", max_bytes=10)


def test_download_error_status_raises_without_url(tmp_path):
    t, _ = transfer(lambda r: httpx.Response(403, content=b"denied"))
    with pytest.raises(TransferError) as e:
        t.download(URL, tmp_path / "input", max_bytes=10)
    assert "403" in str(e.value)
    assert "r2.cloudflarestorage" not in str(e.value) and "secret-sig" not in str(e.value)
    assert e.value.__cause__ is None


def test_download_network_error_raises_without_url(tmp_path):
    def fail(r):
        raise httpx.ConnectError(f"cannot reach {r.url}", request=r)

    t, _ = transfer(fail)
    with pytest.raises(TransferError) as e:
        t.download(URL, tmp_path / "input", max_bytes=10)
    assert "ConnectError" in str(e.value) and "secret-sig" not in str(e.value)
    assert e.value.__cause__ is None and e.value.__suppress_context__


def test_upload_puts_file_with_exact_content_type_and_length(tmp_path):
    src = tmp_path / "out.mp4"
    src.write_bytes(b"m" * 1234)
    t, seen = transfer(lambda r: httpx.Response(200))
    t.upload(URL, src, "video/mp4")
    req = seen[0]
    assert req.method == "PUT" and req.headers["Content-Type"] == "video/mp4"
    assert req.headers["Content-Length"] == "1234" and "Transfer-Encoding" not in req.headers
    assert req.content == b"m" * 1234


def test_upload_error_status_raises_without_url(tmp_path):
    src = tmp_path / "out.mp4"
    src.write_bytes(b"m")
    t, _ = transfer(lambda r: httpx.Response(403))
    with pytest.raises(TransferError) as e:
        t.upload(URL, src, "video/mp4")
    assert "403" in str(e.value) and "secret-sig" not in str(e.value)
