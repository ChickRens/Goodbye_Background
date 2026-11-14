//! JST 基準の日番号。

/// UNIX ミリ秒から JST の日番号を返す。JST 0 時（UTC 15 時）に切り替わる。
pub fn jst_day(now_ms: u64) -> i64 {
    ((now_ms / 1000 + 9 * 3600) / 86400) as i64
}

#[cfg(test)]
mod jst_day_test {
    use super::*;

    #[test]
    fn jst_day_rolls_at_15utc() {
        let d = jst_day(1_700_000_000_000);
        let midnight_jst_ms = ((d * 86400 - 9 * 3600) * 1000) as u64;
        assert_eq!(jst_day(midnight_jst_ms - 1), d - 1);
        assert_eq!(jst_day(midnight_jst_ms), d);
    }
}
