"""cutout の複数のモジュールで共有する例外。"""


class InputTooLarge(Exception):
    """ダウンロード中に入力が max_bytes を超えた（Transfer.download が投げる）。"""
