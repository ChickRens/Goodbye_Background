import hmac
import hashlib
import json

from cutout.notifier import Notifier, sign


def test_sign_matches_known_vector():
    # 署名対象は「{timestamp}.」に続けて本文のバイト列そのもの
    expected = hmac.new(b"secret", b'1700000000.{"a":1}', hashlib.sha256).hexdigest()
    assert sign(b"secret", 1700000000, b'{"a":1}') == expected


def test_progress_is_throttled_to_half_second():
    sent, t = [], [0.0]
    n = Notifier("u", b"s", lambda u, b, h: sent.append(b) or 200, lambda: t[0], lambda s: None)
    n.progress({"p": 1})
    t[0] = 0.3
    n.progress({"p": 2})
    t[0] = 0.6
    n.progress({"p": 3})
    assert len(sent) == 2
    assert json.loads(sent[1]) == {"p": 3}


def test_throttled_calls_do_not_reset_interval():
    # 間引きで捨てた呼び出しは送信時刻の基準を更新しない
    sent, t = [], [0.0]
    n = Notifier("u", b"s", lambda u, b, h: sent.append(b) or 200, lambda: t[0], lambda s: None)
    n.progress({"p": 1})
    t[0] = 0.4
    n.progress({"p": 2})  # 捨てる
    t[0] = 0.5
    n.progress({"p": 3})  # 最後の送信から 0.5 秒経過しているので送る
    assert len(sent) == 2


def test_progress_failure_is_swallowed():
    def boom(*a):
        raise OSError("down")

    n = Notifier("u", b"s", boom, lambda: 0.0, lambda s: None)
    n.progress({"p": 1})  # 例外を出さないこと


def test_progress_non_2xx_is_not_raised():
    n = Notifier("u", b"s", lambda *a: 500, lambda: 0.0, lambda s: None)
    n.progress({"p": 1})


def test_final_retries_with_backoff():
    codes, waits = iter([500, 500, 500, 204]), []
    n = Notifier("u", b"s", lambda *a: next(codes), lambda: 0.0, waits.append)
    assert n.final({"phase": "done"}) is True
    assert waits == [1, 2, 4]


def test_final_first_success_does_not_sleep():
    waits = []
    n = Notifier("u", b"s", lambda *a: 200, lambda: 0.0, waits.append)
    assert n.final({"phase": "done"}) is True
    assert waits == []


def test_final_gives_up_after_four_attempts():
    calls, waits = [], []

    def post(u, b, h):
        calls.append(1)
        return 503

    n = Notifier("u", b"s", post, lambda: 0.0, waits.append)
    assert n.final({"phase": "done"}) is False
    assert len(calls) == 4
    assert waits == [1, 2, 4]


def test_final_exception_counts_as_failure_and_never_raises():
    calls = []

    def post(u, b, h):
        calls.append(1)
        raise OSError("refused")

    n = Notifier("u", b"s", post, lambda: 0.0, lambda s: None)
    assert n.final({"phase": "done"}) is False
    assert len(calls) == 4


def test_final_retry_after_exception_then_success():
    results = iter([OSError("x"), 200])

    def post(u, b, h):
        r = next(results)
        if isinstance(r, Exception):
            raise r
        return r

    waits = []
    n = Notifier("u", b"s", post, lambda: 0.0, waits.append)
    assert n.final({"phase": "done"}) is True
    assert waits == [1]


def test_headers_contain_timestamp_and_signature():
    got = {}
    n = Notifier(
        "u",
        b"s",
        lambda u, b, h: got.update(h=h, b=b, u=u) or 200,
        lambda: 1700000000.4,
        lambda s: None,
    )
    n.final({"phase": "done"})
    assert got["u"] == "u"
    assert got["h"]["X-Signature-Timestamp"] == "1700000000"
    assert got["h"]["X-Signature"] == sign(b"s", 1700000000, got["b"])
    assert got["h"]["Content-Type"] == "application/json"


def test_body_is_compact_json_and_is_what_gets_signed():
    got = {}
    n = Notifier(
        "u",
        b"s",
        lambda u, b, h: got.update(h=h, b=b) or 200,
        lambda: 1700000000.0,
        lambda s: None,
    )
    n.final({"phase": "done", "n": 1})
    assert got["b"] == b'{"phase":"done","n":1}'
    assert got["h"]["X-Signature"] == sign(b"s", 1700000000, got["b"])


def test_signature_timestamp_is_truncated_not_rounded():
    got = {}
    n = Notifier(
        "u",
        b"s",
        lambda u, b, h: got.update(h=h) or 200,
        lambda: 1700000000.999,
        lambda s: None,
    )
    n.final({"phase": "done"})
    assert got["h"]["X-Signature-Timestamp"] == "1700000000"
