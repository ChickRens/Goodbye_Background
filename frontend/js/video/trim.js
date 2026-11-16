/**
 * 選んだ範囲を含む部分だけの MP4 を作る（再エンコードしない。映像・音声のデータは元のファイルの slice をそのまま並べる）。
 * 仕様: docs/superpowers/specs/2026-11-12-video-upload-trim-design.md
 *
 * 使い方: const r = await trimForUpload(file, startMs, endMs);
 *   r が null なら元のファイルを丸ごと送る（読めない形・ほとんど小さくならない・途中で失敗）。
 *   r = { blob, offsetMs } なら blob を送り、ジョブの時刻は全部「元の時刻 − offsetMs」で送る。
 *   ftyp（mp4 / mov の別）は元のファイルのものを写すので、Content-Type は元のファイルと同じでよい。
 *
 * ブラウザでも Node（テスト）でも動くよう、File / Blob の slice と arrayBuffer だけを使う。
 */

/** 範囲の終わりの後ろに余分に含める長さ（ミリ秒）。デコードの順と表示の順のずれ・音声の端を含めるため */
const TAIL_MARGIN_MS = 1000;
/** 切り出しても元の大きさのこの割合以上なら、切らずに元のファイルを送る */
const MIN_SAVING_RATIO = 0.9;
/** 新しいファイルの mvhd の時間の単位（ミリ秒） */
const MOVIE_TIMESCALE = 1000;

class Unsupported extends Error {}

/**
 * @param {Blob} file 元の動画（File）
 * @param {number} startMs 範囲の開始（元の動画のミリ秒）
 * @param {number} endMs 範囲の終わり
 * @returns {Promise<{ blob: Blob, offsetMs: number } | null>}
 */
export async function trimForUpload(file, startMs, endMs) {
  try {
    const { moov, ftyp } = await readTopLevel(file);
    const plan = planTrim(parseMovie(moov), startMs, endMs);
    const blob = buildFile(file, ftyp, plan);
    if (blob.size >= file.size * MIN_SAVING_RATIO) return null;
    return { blob, offsetMs: plan.offsetMs };
  } catch (err) {
    if (!(err instanceof Unsupported)) console.warn('動画の切り出しに失敗。元のファイルを送ります', err);
    return null;
  }
}

/** ジョブに送る時刻（元の時刻 − offsetMs。offsetMs は整数なので、時刻どうしの差は変わらない） */
export function toJobTime(timeMs, offsetMs) {
  return Math.round(timeMs) - offsetMs;
}

// ---------- 読む ----------

async function bytes(file, start, end) {
  return new DataView(await file.slice(start, end).arrayBuffer());
}

/** 先頭から箱を順に見て moov と ftyp を読む（moov は先頭にも末尾にもありうる）。断片化 MP4（moof）は扱わない */
async function readTopLevel(file) {
  let pos = 0;
  let moov = null;
  let ftyp = null;
  while (pos + 8 <= file.size) {
    const h = await bytes(file, pos, Math.min(pos + 16, file.size));
    let size = h.getUint32(0);
    const type = fourcc(h, 4);
    let header = 8;
    if (size === 1) {
      size = Number(h.getBigUint64(8));
      header = 16;
    } else if (size === 0) {
      size = file.size - pos;
    }
    if (size < header) throw new Unsupported('箱の大きさが不正');
    if (type === 'moof') throw new Unsupported('断片化 MP4');
    if (type === 'moov') moov = await bytes(file, pos, pos + size);
    if (type === 'ftyp' && !ftyp) ftyp = new Uint8Array(await file.slice(pos, pos + size).arrayBuffer());
    pos += size;
  }
  if (!moov) throw new Unsupported('moov が無い');
  return { moov, ftyp };
}

function fourcc(view, at) {
  return String.fromCharCode(view.getUint8(at), view.getUint8(at + 1), view.getUint8(at + 2), view.getUint8(at + 3));
}

/** view の [start, end) にある箱の一覧 */
function boxes(view, start, end) {
  const out = [];
  let pos = start;
  while (pos + 8 <= end) {
    let size = view.getUint32(pos);
    let header = 8;
    if (size === 1) {
      size = Number(view.getBigUint64(pos + 8));
      header = 16;
    } else if (size === 0) {
      size = end - pos;
    }
    if (size < header || pos + size > end) throw new Unsupported('箱の大きさが不正');
    out.push({ type: fourcc(view, pos + 4), start: pos, body: pos + header, end: pos + size });
    pos += size;
  }
  return out;
}

function child(view, box, type) {
  return boxes(view, box.body, box.end).find((b) => b.type === type) ?? null;
}

function need(box, what) {
  if (!box) throw new Unsupported(`${what} が無い`);
  return box;
}

function raw(view, box) {
  return new Uint8Array(view.buffer, view.byteOffset + box.start, box.end - box.start).slice();
}

/** moov から、最初の映像トラックと最初の音声トラックのサンプル表を取り出す */
export function parseMovie(view) {
  const moov = boxes(view, 0, view.byteLength)[0];
  const children = boxes(view, moov.body, moov.end);
  if (children.some((b) => b.type === 'mvex' || b.type === 'cmov')) throw new Unsupported('断片化・圧縮された moov');
  const tracks = { video: null, audio: null };
  for (const trak of children.filter((b) => b.type === 'trak')) {
    const mdia = need(child(view, trak, 'mdia'), 'mdia');
    const hdlr = need(child(view, mdia, 'hdlr'), 'hdlr');
    const handler = fourcc(view, hdlr.body + 8);
    const kind = handler === 'vide' ? 'video' : handler === 'soun' ? 'audio' : null;
    if (!kind || tracks[kind]) continue;
    tracks[kind] = parseTrack(view, trak, mdia, hdlr);
  }
  if (!tracks.video) throw new Unsupported('映像トラックが無い');
  return tracks;
}

function parseTrack(view, trak, mdia, hdlr) {
  const tkhd = need(child(view, trak, 'tkhd'), 'tkhd');
  const mdhd = need(child(view, mdia, 'mdhd'), 'mdhd');
  const minf = need(child(view, mdia, 'minf'), 'minf');
  const stbl = need(child(view, minf, 'stbl'), 'stbl');
  const mediaHeader = child(view, minf, 'vmhd') ?? child(view, minf, 'smhd');
  const t = (type) => child(view, stbl, type);

  const mdhdV1 = view.getUint8(mdhd.body) === 1;
  const timescale = view.getUint32(mdhd.body + (mdhdV1 ? 20 : 12));
  const language = view.getUint16(mdhd.body + (mdhdV1 ? 32 : 20));
  if (!timescale) throw new Unsupported('timescale が 0');
  if (t('stz2')) throw new Unsupported('stz2');

  const durations = runLengths(view, need(t('stts'), 'stts'));
  const n = durations.length;
  const ctts = t('ctts');
  const ctsOffsets = ctts ? runLengths(view, ctts, true) : new Array(n).fill(0);
  const sizes = sampleSizes(view, need(t('stsz'), 'stsz'));
  const offsets = sampleOffsets(view, need(t('stsc'), 'stsc'), t('stco') ?? need(t('co64'), 'stco'), sizes);
  if (ctsOffsets.length !== n || sizes.length !== n || offsets.length !== n) throw new Unsupported('サンプル表の数が合わない');
  const stss = t('stss');
  const sync = stss ? syncFlags(view, stss, n) : new Array(n).fill(true);

  return {
    tkhd: parseTkhd(view, tkhd),
    timescale,
    language,
    mediaTime: editMediaTime(view, child(view, trak, 'edts')),
    hdlr: raw(view, hdlr),
    mediaHeader: mediaHeader ? raw(view, mediaHeader) : null,
    stsd: raw(view, need(t('stsd'), 'stsd')),
    durations,
    ctsOffsets,
    sizes,
    offsets,
    sync,
  };
}

/** stts / ctts の (回数, 値) の表を、サンプルごとの値の配列にする（ctts の version 1 は符号付き） */
function runLengths(view, box, signedIfV1 = false) {
  const signed = signedIfV1 && view.getUint8(box.body) === 1;
  const count = view.getUint32(box.body + 4);
  const out = [];
  for (let i = 0; i < count; i++) {
    const at = box.body + 8 + i * 8;
    const times = view.getUint32(at);
    const value = signed ? view.getInt32(at + 4) : view.getUint32(at + 4);
    for (let k = 0; k < times; k++) out.push(value);
  }
  return out;
}

function sampleSizes(view, box) {
  const fixed = view.getUint32(box.body + 4);
  const count = view.getUint32(box.body + 8);
  if (fixed) return new Array(count).fill(fixed);
  return Array.from({ length: count }, (_, i) => view.getUint32(box.body + 12 + i * 4));
}

/** stsc（チャンクごとのサンプル数）と stco / co64（チャンクの位置）から、サンプルごとのファイル内の位置を出す */
function sampleOffsets(view, stsc, stco, sizes) {
  const wide = stco.type === 'co64';
  const chunkCount = view.getUint32(stco.body + 4);
  const chunk = (i) => (wide ? Number(view.getBigUint64(stco.body + 8 + i * 8)) : view.getUint32(stco.body + 8 + i * 4));
  const entries = view.getUint32(stsc.body + 4);
  const runs = Array.from({ length: entries }, (_, i) => {
    const at = stsc.body + 8 + i * 12;
    return { first: view.getUint32(at) - 1, perChunk: view.getUint32(at + 4), desc: view.getUint32(at + 8) };
  });
  if (runs.some((r) => r.desc !== 1)) throw new Unsupported('サンプルの説明が複数');
  const out = [];
  for (let r = 0; r < runs.length; r++) {
    const last = r + 1 < runs.length ? runs[r + 1].first : chunkCount;
    for (let c = runs[r].first; c < last; c++) {
      let pos = chunk(c);
      for (let k = 0; k < runs[r].perChunk && out.length < sizes.length; k++) {
        out.push(pos);
        pos += sizes[out.length - 1];
      }
    }
  }
  return out;
}

function syncFlags(view, box, n) {
  const flags = new Array(n).fill(false);
  const count = view.getUint32(box.body + 4);
  for (let i = 0; i < count; i++) {
    const k = view.getUint32(box.body + 8 + i * 4) - 1;
    if (k >= 0 && k < n) flags[k] = true;
  }
  return flags;
}

/** 編集リストの開始位置（メディアの時間の単位）。無い・1 区間だけなら扱う。空の区間や複数区間は扱わない */
function editMediaTime(view, edts) {
  if (!edts) return 0;
  const elst = child(view, edts, 'elst');
  if (!elst) return 0;
  const v1 = view.getUint8(elst.body) === 1;
  const count = view.getUint32(elst.body + 4);
  if (count === 0) return 0;
  if (count !== 1) throw new Unsupported('編集リストが複数区間');
  const at = elst.body + 8;
  const mediaTime = v1 ? Number(view.getBigInt64(at + 8)) : view.getInt32(at + 4);
  const rate = view.getInt16(at + (v1 ? 16 : 8));
  if (mediaTime < 0 || rate !== 1) throw new Unsupported('空の区間・速度の違う編集リスト');
  return mediaTime;
}

function parseTkhd(view, box) {
  const v1 = view.getUint8(box.body) === 1;
  const flags = view.getUint32(box.body) & 0xffffff;
  const b = box.body + 4;
  const id = view.getUint32(b + (v1 ? 16 : 8));
  const rest = b + (v1 ? 32 : 20); // reserved(8) の位置
  return {
    flags,
    id,
    layer: view.getInt16(rest + 8),
    altGroup: view.getInt16(rest + 10),
    volume: view.getInt16(rest + 12),
    matrix: new Uint8Array(view.buffer, view.byteOffset + rest + 16, 36).slice(),
    width: view.getUint32(rest + 52),
    height: view.getUint32(rest + 56),
  };
}

// ---------- 切る位置を決める ----------

/** サンプルごとの表示時刻（ミリ秒、元の動画の時刻）と、デコードの時刻 */
function timesOf(track) {
  const dts = new Array(track.durations.length);
  let t = 0;
  for (let i = 0; i < dts.length; i++) {
    dts[i] = t;
    t += track.durations[i];
  }
  const ms = (v) => ((v - track.mediaTime) / track.timescale) * 1000;
  return { dts, presMs: dts.map((d, i) => ms(d + track.ctsOffsets[i])), dtsMs: dts.map(ms) };
}

/**
 * どのサンプルを残すか。映像は「表示時刻が開始以下の、最後のキーフレーム」K から、デコードの時刻が終わり + 余白までを残す。
 * 新しいファイルの時刻 0 = K の表示時刻（元の時刻 O ミリ秒）。offsetMs = round(O)。
 */
export function planTrim(tracks, startMs, endMs) {
  const video = tracks.video;
  const vt = timesOf(video);
  let first = -1;
  for (let i = 0; i < vt.presMs.length; i++) {
    if (video.sync[i] && vt.presMs[i] <= startMs + 1e-6 && (first < 0 || vt.presMs[i] >= vt.presMs[first])) first = i;
  }
  if (first < 0) first = video.sync.indexOf(true);
  if (first < 0) throw new Unsupported('キーフレームが無い');
  const limit = endMs + TAIL_MARGIN_MS;
  let last = first;
  while (last + 1 < vt.dtsMs.length && vt.dtsMs[last + 1] <= limit) last++;

  const originMs = vt.presMs[first];
  const out = {
    offsetMs: Math.round(originMs),
    tracks: [
      sliceTrack(video, vt, first, last, video.ctsOffsets[first]),
    ],
  };
  if (tracks.audio) {
    const audio = tracks.audio;
    const at = timesOf(audio);
    let a0 = 0;
    for (let i = 0; i < at.presMs.length; i++) if (at.presMs[i] <= originMs + 1e-6) a0 = i;
    let a1 = a0;
    while (a1 + 1 < at.presMs.length && at.presMs[a1 + 1] <= limit) a1++;
    // 新しい時刻 0 に当たる音声のメディア時刻: 元の時刻 O → (O/1000)·ts + 元の編集リストの開始 − 残す先頭の dts
    const mediaTime = Math.round((originMs / 1000) * audio.timescale + audio.mediaTime - at.dts[a0]);
    out.tracks.push(sliceTrack(audio, at, a0, a1, Math.max(0, mediaTime)));
  }
  return out;
}

function sliceTrack(track, times, from, to, mediaTime) {
  const range = (arr) => arr.slice(from, to + 1);
  const durations = range(track.durations);
  const mediaDuration = durations.reduce((s, d) => s + d, 0);
  // 表示の終わり（最後に表示されるサンプルの終わり）。B フレームのある映像では dts の合計より ctts の分だけ後ろになる
  let presentationEnd = 0;
  for (let i = from; i <= to; i++) {
    presentationEnd = Math.max(presentationEnd, times.dts[i] - times.dts[from] + track.ctsOffsets[i] + track.durations[i]);
  }
  return {
    ...track,
    durations,
    ctsOffsets: range(track.ctsOffsets),
    sizes: range(track.sizes),
    offsets: range(track.offsets),
    sync: range(track.sync),
    mediaTime,
    // 編集リストの長さ（新しいファイルの mvhd の単位 = ミリ秒）
    editDuration: Math.max(1, Math.round(((presentationEnd - mediaTime) / track.timescale) * MOVIE_TIMESCALE)),
    mediaDuration,
  };
}

// ---------- 書く ----------

class Writer {
  constructor() {
    this.parts = [];
  }
  u8(v) { this.parts.push(Uint8Array.of(v)); return this; }
  u16(v) { const b = new DataView(new ArrayBuffer(2)); b.setUint16(0, v); this.parts.push(new Uint8Array(b.buffer)); return this; }
  i16(v) { const b = new DataView(new ArrayBuffer(2)); b.setInt16(0, v); this.parts.push(new Uint8Array(b.buffer)); return this; }
  u32(v) { const b = new DataView(new ArrayBuffer(4)); b.setUint32(0, v >>> 0); this.parts.push(new Uint8Array(b.buffer)); return this; }
  i32(v) { const b = new DataView(new ArrayBuffer(4)); b.setInt32(0, v); this.parts.push(new Uint8Array(b.buffer)); return this; }
  u64(v) { const b = new DataView(new ArrayBuffer(8)); b.setBigUint64(0, BigInt(v)); this.parts.push(new Uint8Array(b.buffer)); return this; }
  i64(v) { const b = new DataView(new ArrayBuffer(8)); b.setBigInt64(0, BigInt(v)); this.parts.push(new Uint8Array(b.buffer)); return this; }
  str(s) { this.parts.push(Uint8Array.from(s, (c) => c.charCodeAt(0))); return this; }
  bytes(b) { this.parts.push(b); return this; }
  zeros(n) { this.parts.push(new Uint8Array(n)); return this; }
  done() {
    const len = this.parts.reduce((s, p) => s + p.length, 0);
    const out = new Uint8Array(len);
    let pos = 0;
    for (const p of this.parts) {
      out.set(p, pos);
      pos += p.length;
    }
    return out;
  }
}

function box(type, ...payload) {
  const body = new Writer();
  for (const p of payload) body.bytes(p);
  const b = body.done();
  return new Writer().u32(8 + b.length).str(type).bytes(b).done();
}

function fullBox(type, version, flags, fill) {
  const w = new Writer().u8(version).u8((flags >> 16) & 0xff).u16(flags & 0xffff);
  fill(w);
  return box(type, w.done());
}

const IDENTITY = (() => {
  const w = new Writer();
  for (const v of [0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]) w.u32(v);
  return w.done();
})();

function buildMoov(plan, chunkOffsets) {
  const duration = Math.max(...plan.tracks.map((t) => t.editDuration));
  const mvhd = fullBox('mvhd', 1, 0, (w) =>
    w.u64(0).u64(0).u32(MOVIE_TIMESCALE).u64(duration).u32(0x00010000).u16(0x0100).zeros(10).bytes(IDENTITY).zeros(24)
      .u32(plan.tracks.length + 1),
  );
  const traks = plan.tracks.map((t, i) => buildTrak(t, i + 1, chunkOffsets[i]));
  return box('moov', mvhd, ...traks);
}

function buildTrak(t, id, chunkOffset) {
  const tkhd = fullBox('tkhd', 1, t.tkhd.flags | 1, (w) =>
    w.u64(0).u64(0).u32(id).u32(0).u64(t.editDuration).zeros(8).i16(t.tkhd.layer).i16(t.tkhd.altGroup)
      .i16(t.tkhd.volume).u16(0).bytes(t.tkhd.matrix).u32(t.tkhd.width).u32(t.tkhd.height),
  );
  const elst = fullBox('elst', 1, 0, (w) => w.u32(1).u64(t.editDuration).i64(t.mediaTime).i16(1).i16(0));
  const mdhd = fullBox('mdhd', 1, 0, (w) => w.u64(0).u64(0).u32(t.timescale).u64(t.mediaDuration).u16(t.language).u16(0));
  const dinf = box('dinf', fullBox('dref', 0, 0, (w) => w.u32(1).bytes(fullBox('url ', 0, 1, () => {}))));
  const minf = box('minf', ...(t.mediaHeader ? [t.mediaHeader] : []), dinf, buildStbl(t, chunkOffset));
  return box('trak', tkhd, box('edts', elst), box('mdia', mdhd, t.hdlr, minf));
}

function buildStbl(t, chunkOffset) {
  const n = t.durations.length;
  const runs = (values) => {
    const out = [];
    for (const v of values) {
      if (out.length && out[out.length - 1][1] === v) out[out.length - 1][0]++;
      else out.push([1, v]);
    }
    return out;
  };
  const stts = runs(t.durations);
  const parts = [
    t.stsd,
    fullBox('stts', 0, 0, (w) => { w.u32(stts.length); for (const [c, v] of stts) w.u32(c).u32(v); }),
  ];
  if (t.ctsOffsets.some((v) => v !== 0)) {
    const ctts = runs(t.ctsOffsets);
    const negative = t.ctsOffsets.some((v) => v < 0);
    parts.push(fullBox('ctts', negative ? 1 : 0, 0, (w) => { w.u32(ctts.length); for (const [c, v] of ctts) w.u32(c).i32(v); }));
  }
  if (!t.sync.every(Boolean)) {
    const idx = t.sync.flatMap((s, i) => (s ? [i + 1] : []));
    parts.push(fullBox('stss', 0, 0, (w) => { w.u32(idx.length); for (const k of idx) w.u32(k); }));
  }
  parts.push(
    fullBox('stsc', 0, 0, (w) => w.u32(1).u32(1).u32(n).u32(1)), // 1 トラック = 1 チャンク
    fullBox('stsz', 0, 0, (w) => { w.u32(0).u32(n); for (const s of t.sizes) w.u32(s); }),
    fullBox('co64', 0, 0, (w) => w.u32(1).u64(chunkOffset)),
  );
  return box('stbl', ...parts);
}

/** [ftyp][moov][mdat（64 bit の大きさ）][映像のサンプル][音声のサンプル]。サンプルは元のファイルの slice のまま */
function buildFile(file, originalFtyp, plan) {
  // mov と mp4 で音声の説明の読み方が違う所があるので、元の ftyp をそのまま使う（無い古い mov は QuickTime として書く）
  const ftyp = originalFtyp ?? box('ftyp', new Writer().str('qt  ').u32(0x200).str('qt  ').done());
  const trackBytes = plan.tracks.map((t) => t.sizes.reduce((s, v) => s + v, 0));
  // co64 は 8 バイト固定なので、仮の位置で moov の大きさを測ってから本当の位置を入れる
  const moovSize = buildMoov(plan, plan.tracks.map(() => 0)).length;
  const dataStart = ftyp.length + moovSize + 16;
  const chunkOffsets = trackBytes.map((_, i) => dataStart + trackBytes.slice(0, i).reduce((s, v) => s + v, 0));
  const moov = buildMoov(plan, chunkOffsets);
  const total = trackBytes.reduce((s, v) => s + v, 0);
  const mdatHeader = new Writer().u32(1).str('mdat').u64(16 + total).done();

  const parts = [ftyp, moov, mdatHeader];
  for (const t of plan.tracks) {
    let a = -1;
    let b = -1;
    for (let i = 0; i < t.sizes.length; i++) {
      const s = t.offsets[i];
      const e = s + t.sizes[i];
      if (s === b) {
        b = e; // 前のサンプルと続いている → まとめて 1 つの slice にする
      } else {
        if (a >= 0) parts.push(file.slice(a, b));
        a = s;
        b = e;
      }
    }
    if (a >= 0) parts.push(file.slice(a, b));
  }
  return new Blob(parts, { type: file.type || 'application/octet-stream' });
}
