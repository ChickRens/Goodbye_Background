/**
 * R2 の署名付き URL へ動画を PUT する（進捗を出したいので fetch ではなく XMLHttpRequest）。
 *
 * - Content-Type は署名の一部。POST /video-jobs で申告した文字列と完全に同じものを渡すこと。
 * - Content-Length も署名の一部。ブラウザが File から自動で付ける（= file.size）ので、
 *   POST /video-jobs の size_bytes には必ず file.size をそのまま申告すること（違うと R2 が 403 を返す）。
 * - R2 は別オリジンなので client-id などの独自ヘッダーは付けない（署名にも CORS にも含まれない）。
 * - 返り値は { promise, abort }。やり直し（別の動画を選ぶ等）のときに abort で送信を止める。
 */
export function putWithProgress(url, file, contentType, onProgress) {
  const xhr = new XMLHttpRequest();
  const promise = new Promise((resolve, reject) => {
    xhr.open('PUT', url);
    xhr.setRequestHeader('Content-Type', contentType);

    xhr.upload.addEventListener('progress', (evt) => {
      if (evt.lengthComputable && onProgress) onProgress(evt.loaded / evt.total);
    });
    xhr.addEventListener('load', () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        if (onProgress) onProgress(1);
        resolve();
      } else {
        reject(new Error(`アップロードに失敗しました（HTTP ${xhr.status}）`));
      }
    });
    xhr.addEventListener('error', () => reject(new Error('アップロード中に通信が切れました')));
    xhr.addEventListener('abort', () => reject(new Error('アップロードが中断されました')));

    xhr.send(file);
  });
  return { promise, abort: () => xhr.abort() };
}
