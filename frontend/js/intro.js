/*
 * 機能紹介ページ(index.html)のオープニング演出と背景動画ループ。
 *
 *   1. アクセス直後: sample1 → 2 → 3 を順に画面いっぱいに再生する
 *      （この間、ホーム画面のテキストはまだ隠している）
 *   2. sample3 の再生が終わったら: ホーム画面を前面にフェードインし、
 *      背景は明るさを大きく落として sample1, 2, 3, 1, 2, 3... と循環再生を続ける
 *
 * 動画は 1:1。画面の短辺にぴったり合わせて表示し、足りない左右(または上下)は
 * 同じ動画を拡大してぼかしたものでうめる。
 *
 * 読み込みを軽くするための工夫:
 *   - くっきり用(sharp)は切り替えの隙間を作らないよう全部 preload="auto" で先読みする。
 *   - ぼかし用(blur)は縁を埋めるだけの補助なので、最初の1本以外は
 *     preload="metadata" にして、再生が始まるまで本体のダウンロードを遅らせる。
 *     （動画3本ぶん × 2枚 = 6本を一斉に読み込むのをやめ、帯域を sharp に回す）
 *
 * 背景の暗さは css/landing.css の --bg-dim で調整できる。
 */
(() => {
    'use strict';

    // 再生順。全部を順に1回ずつフルスクリーンで再生したあと(=オープニング)、
    // そのままの順で循環しつづけ、背景になる。
    const SOURCES = [
        'assets/sample1.mp4',
        'assets/sample2.mp4',
        'assets/sample3.mp4',
    ];

    // 最初の動画が再生を始められない(読み込みが遅い等)場合に演出を諦めるまでの時間
    const START_TIMEOUT_MS = 3500;
    // 何があってもホーム画面を出す、演出全体の保険タイムアウト
    // （動画3本で約3秒 + 読み込み待ちの余裕）
    const INTRO_MAX_MS = 9000;
    // 連続でこの回数失敗したら背景ループを止める（読み込み失敗の無限ループ防止）
    const MAX_CONSECUTIVE_FAILURES = 3;

    const root = document.documentElement;
    const stage = document.getElementById('bg-stage');
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    let introDone = false;
    let startTimer = null;
    let maxTimer = null;

    // ホーム画面を前面に出し、背景を暗くする。何度呼んでも1回しか効かない。
    function finishIntro() {
        if (introDone) return;
        introDone = true;
        clearTimeout(startTimer);
        clearTimeout(maxTimer);
        root.classList.remove('intro-pending');
        if (stage) stage.classList.add('is-dimmed');
    }

    // 動画を使わない(できない)場合でも、ホーム画面は必ず表示する
    if (!stage || reduceMotion) {
        finishIntro();
        return;
    }

    function createVideo(src, className, preload) {
        const v = document.createElement('video');
        v.className = `bg-video ${className}`;
        v.src = src;
        // 自動再生には muted + playsinline が必須（特にiOS Safari）
        v.muted = true;
        v.defaultMuted = true;
        v.setAttribute('muted', '');
        v.playsInline = true;
        v.setAttribute('playsinline', '');
        v.preload = preload;
        v.disablePictureInPicture = true;
        v.tabIndex = -1;
        v.setAttribute('aria-hidden', 'true');
        return v;
    }

    // 動画ごとに「ぼかし用」と「くっきり用」の2枚をセットで持つ。
    // くっきり用は全部先に読み込んでおき、切り替え時に読み込み待ちが出ないようにする。
    // ぼかし用は最初の1本だけ先読みし、残りはメタデータだけ読んでおく。
    const slots = SOURCES.map((src, index) => {
        const el = document.createElement('div');
        el.className = 'bg-slot';
        const blur = createVideo(src, 'bg-video-blur', index === 0 ? 'auto' : 'metadata');
        const sharp = createVideo(src, 'bg-video-sharp', 'auto');
        el.append(blur, sharp);
        stage.append(el);
        return { el, blur, sharp };
    });

    let current = -1;
    let zCounter = 1;
    let failures = 0;
    let stopped = false;

    // 再生を終えたスロットを非表示にして、次に使えるよう先頭へ巻き戻しておく
    function retire(slot) {
        slot.el.classList.remove('is-active');
        for (const v of [slot.blur, slot.sharp]) {
            v.pause();
            v.currentTime = 0;
        }
    }

    function handleFailure(err) {
        failures += 1;
        // 自動再生をブロックされた場合は、何度やっても同じなのですぐ諦める
        const blocked = err && err.name === 'NotAllowedError';
        if (blocked || failures >= MAX_CONSECUTIVE_FAILURES) {
            stopped = true;
            finishIntro();
            return;
        }
        // 最初の動画が駄目でもホーム画面は出し、次の動画へ進む
        finishIntro();
        showSlot((current + 1) % slots.length);
    }

    function showSlot(index) {
        if (stopped) return;

        const slot = slots[index];
        const prev = current >= 0 && current !== index ? slots[current] : null;
        current = index;

        // 新しい動画を最前面に出してから再生を始め、再生が始まってから前の動画を片付ける。
        // こうすると切り替わる瞬間に背景が一瞬空になるのを防げる。
        slot.el.style.zIndex = String(++zCounter);
        slot.el.classList.add('is-active');

        // ぼかし側は再生を始めるときに本体の読み込みも始まる（preload="metadata" の場合）
        slot.blur.play().catch(() => { /* ぼかし側の失敗は無視してよい */ });
        slot.sharp.play().then(() => {
            failures = 0;
            if (index === 0 && !introDone) clearTimeout(startTimer);
            if (prev) retire(prev);
        }).catch(handleFailure);
    }

    slots.forEach((slot, index) => {
        slot.sharp.addEventListener('ended', () => {
            if (current !== index) return;
            // 最後の動画(sample3)の再生が終わった瞬間にホーム画面へ切り替える。
            // それまでの動画はフルスクリーンのまま次へ進む。
            if (index === slots.length - 1) finishIntro();
            showSlot((index + 1) % slots.length);
        });
        slot.sharp.addEventListener('error', () => {
            if (current === index) handleFailure();
        });
    });

    // 最初の動画が始まらないまま待たされ続けないようにする
    startTimer = setTimeout(finishIntro, START_TIMEOUT_MS);
    maxTimer = setTimeout(finishIntro, INTRO_MAX_MS);

    showSlot(0);
})();