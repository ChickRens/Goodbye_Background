/*
 * 候補C(home-c.html)のお試しデモ。
 * 元写真の上に置いた見えないボタン(.demo-hit)を押すと、対応する切り抜き結果の画像を表示する。
 * サーバーには何も送らない（結果は事前に用意した assets/after1〜3.png を出し分けているだけ）。
 */
(() => {
    'use strict';
    const demo = document.querySelector('[data-demo]');
    if (!demo) return;

    const photo = demo.querySelector('.demo-photo');
    const marker = demo.querySelector('.demo-marker');
    const empty = demo.querySelector('.demo-empty');
    const results = demo.querySelectorAll('[data-result]');

    function show(key) {
        empty.hidden = true;
        results.forEach((img) => { img.hidden = img.dataset.result !== key; });
    }

    demo.querySelectorAll('.demo-hit').forEach((hit) => {
        hit.addEventListener('click', (e) => {
            // タップした位置に印を置く（キーボード操作のときはボタンの中心）
            const box = photo.getBoundingClientRect();
            const r = hit.getBoundingClientRect();
            const x = e.clientX || r.left + r.width / 2;
            const y = e.clientY || r.top + r.height / 2;
            marker.style.left = `${x - box.left}px`;
            marker.style.top = `${y - box.top}px`;
            marker.hidden = false;
            show(hit.dataset.after);
        });
    });
})();
