use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
let _mediaRecorder = null;
let _chunks = [];
let _stream = null;
let _recordStart = 0;
let _graph = null;
let _currentLinkMode = false;
let _pendingLink = null;
let _highlightedNode = null;

let _audioCtx = null;
let _audioPlayers = new Map();

function _log(...args) { console.log('[notepunk]', ...args); }
function _err(...args) { console.error('[notepunk]', ...args); }

function _ensureCtx() {
    if (!_audioCtx) _audioCtx = new (window.AudioContext || window.webkitAudioContext)();
    if (_audioCtx.state === 'suspended') _audioCtx.resume();
    return _audioCtx;
}

const _DB_NAME = 'notepunk-audio';
const _DB_STORE = 'tracks';
let _dbPromise = null;
function _openDB() {
    if (_dbPromise) return _dbPromise;
    _dbPromise = new Promise((resolve, reject) => {
        const req = indexedDB.open(_DB_NAME, 1);
        req.onupgradeneeded = () => {
            const db = req.result;
            if (!db.objectStoreNames.contains(_DB_STORE)) db.createObjectStore(_DB_STORE);
        };
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
    });
    return _dbPromise;
}

async function _idbPut(id, blob) {
    const db = await _openDB();
    return new Promise((resolve, reject) => {
        const tx = db.transaction(_DB_STORE, 'readwrite');
        tx.objectStore(_DB_STORE).put(blob, id);
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
    });
}

async function _idbGet(id) {
    const db = await _openDB();
    return new Promise((resolve, reject) => {
        const tx = db.transaction(_DB_STORE, 'readonly');
        const req = tx.objectStore(_DB_STORE).get(id);
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
    });
}

async function _idbDelete(id) {
    const db = await _openDB();
    return new Promise((resolve, reject) => {
        const tx = db.transaction(_DB_STORE, 'readwrite');
        tx.objectStore(_DB_STORE).delete(id);
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
    });
}

export async function audioStartRecording() {
    if (_mediaRecorder && _mediaRecorder.state === 'recording') {
        return { ok: false, error: 'already recording' };
    }
    try {
        _stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        _chunks = [];
        _mediaRecorder = new MediaRecorder(_stream);
        _mediaRecorder.ondataavailable = (e) => {
            if (e.data && e.data.size > 0) _chunks.push(e.data);
        };
        _recordStart = performance.now();
        _mediaRecorder.start();
        return { ok: true };
    } catch (e) {
        _err('audioStartRecording:', e);
        return { ok: false, error: 'mic: ' + (e && e.message ? e.message : String(e)) };
    }
}

export async function audioStopRecording(trackId) {
    if (!_mediaRecorder || _mediaRecorder.state !== 'recording') {
        return { ok: false, error: 'not recording' };
    }
    return new Promise((resolve) => {
        _mediaRecorder.onstop = async () => {
            try {
                if (_stream) _stream.getTracks().forEach(t => t.stop());
                const duration = performance.now() - _recordStart;
                const blob = new Blob(_chunks, { type: 'audio/webm' });
                if (blob.size === 0) {
                    resolve({ ok: false, error: 'no audio recorded' });
                    return;
                }
                await _idbPut(trackId, blob);
                resolve({ ok: true, duration_ms: duration });
            } catch (e) {
                _err('audioStopRecording:', e);
                resolve({ ok: false, error: 'save: ' + (e && e.message ? e.message : String(e)) });
            }
        };
        _mediaRecorder.stop();
    });
}

async function _decode(trackId) {
    const blob = await _idbGet(trackId);
    if (!blob) throw new Error('track missing');
    const buf = await blob.arrayBuffer();
    const ctx = _ensureCtx();
    return await ctx.decodeAudioData(buf.slice(0));
}

export async function audioPlay(trackId, looping, volume) {
    try {
        const existing = _audioPlayers.get(trackId);
        if (existing) {
            try { existing.source.stop(); } catch (_) {}
            _audioPlayers.delete(trackId);
        }
        const ctx = _ensureCtx();
        const audioBuffer = await _decode(trackId);
        const source = ctx.createBufferSource();
        source.buffer = audioBuffer;
        source.loop = !!looping;
        const gain = ctx.createGain();
        gain.gain.value = Math.max(0, Math.min(1, volume));
        source.connect(gain).connect(ctx.destination);
        source.start();
        const onEnded = () => {
            _audioPlayers.delete(trackId);
            window.dispatchEvent(new CustomEvent('notepunk-track-ended', { detail: { id: trackId } }));
        };
        source.onended = onEnded;
        _audioPlayers.set(trackId, { source, gain });
        return { ok: true };
    } catch (e) {
        _err('audioPlay:', e);
        return { ok: false, error: 'play: ' + (e && e.message ? e.message : String(e)) };
    }
}

export function audioStop(trackId) {
    const p = _audioPlayers.get(trackId);
    if (!p) return { ok: true };
    try { p.source.stop(); } catch (_) {}
    _audioPlayers.delete(trackId);
    return { ok: true };
}

export function audioStopAll() {
    for (const [id, p] of _audioPlayers) {
        try { p.source.stop(); } catch (_) {}
    }
    _audioPlayers.clear();
    return { ok: true };
}

export function audioSetVolume(trackId, volume) {
    const p = _audioPlayers.get(trackId);
    if (!p) return { ok: true };
    const ctx = _ensureCtx();
    p.gain.gain.setValueAtTime(Math.max(0, Math.min(1, volume)), ctx.currentTime);
    return { ok: true };
}

export async function audioPlayMix(trackJson) {
    try {
        const tracks = JSON.parse(trackJson);
        audioStopAll();
        const ctx = _ensureCtx();
        for (const t of tracks) {
            const audioBuffer = await _decode(t.id);
            const source = ctx.createBufferSource();
            source.buffer = audioBuffer;
            source.loop = !!t.looping;
            const gain = ctx.createGain();
            gain.gain.value = Math.max(0, Math.min(1, t.volume));
            source.connect(gain).connect(ctx.destination);
            source.start();
            source.onended = () => {
                _audioPlayers.delete(t.id);
                window.dispatchEvent(new CustomEvent('notepunk-track-ended', { detail: { id: t.id } }));
            };
            _audioPlayers.set(t.id, { source, gain });
        }
        return { ok: true };
    } catch (e) {
        _err('audioPlayMix:', e);
        return { ok: false, error: 'mix: ' + (e && e.message ? e.message : String(e)) };
    }
}

export async function audioDelete(trackId) {
    audioStop(trackId);
    try {
        await _idbDelete(trackId);
        return { ok: true };
    } catch (e) {
        return { ok: false, error: String(e) };
    }
}

function _edgeColor(kind) {
    switch (kind) {
        case 'wikilink':   return '#b8442a';
        case 'tag':        return '#2a4d8f';
        case 'similarity': return '#7a6f5c';
        case 'manual':     return '#1a1612';
        case 'auto':       return '#c89b1a';
        default:           return '#1a1612';
    }
}

function _edgeDash(kind) {
    switch (kind) {
        case 'tag':        return [2, 2];
        case 'similarity': return [1, 3];
        case 'auto':       return [4, 4];
        default:           return null;
    }
}

function _nodeColor(degree, recency, maxDegree, skillLevel) {
    const d = maxDegree > 0 ? Math.min(1, degree / maxDegree) : 0;
    const heat = 0.35 * recency + 0.65 * d;
    let r = 244 + (184 - 244) * heat;
    let g = 234 + (68  - 234) * heat;
    let b = 213 + (42  - 213) * heat;
    if (skillLevel && skillLevel > 0) {
        const k = Math.min(1, skillLevel / 12);
        r = r + (230 - r) * k;
        g = g + (180 - g) * k;
        b = b + (40  - b) * k;
    }
    return 'rgb(' + Math.round(r) + ',' + Math.round(g) + ',' + Math.round(b) + ')';
}

export function sfxPlay(name) {
    try {
        if (window._notepunkMuted) return;
        const ctx = _ensureCtx();
        const now = ctx.currentTime;
        const presets = {
            xp:     [[880, 0.05, 'square'], [1320, 0.08, 'square']],
            level:  [[523, 0.07, 'square'], [659, 0.07, 'square'], [784, 0.07, 'square'], [1047, 0.18, 'square']],
            quest:  [[659, 0.06, 'triangle'], [784, 0.06, 'triangle'], [988, 0.12, 'triangle']],
            gold:   [[1568, 0.04, 'square'], [2093, 0.06, 'square']],
            tap:    [[440, 0.03, 'square']],
            err:    [[220, 0.12, 'sawtooth'], [165, 0.16, 'sawtooth']],
        };
        const seq = presets[name] || presets.xp;
        let t = now;
        for (const [freq, dur, type] of seq) {
            const osc = ctx.createOscillator();
            const gain = ctx.createGain();
            osc.type = type;
            osc.frequency.value = freq;
            gain.gain.setValueAtTime(0, t);
            gain.gain.linearRampToValueAtTime(0.06, t + 0.005);
            gain.gain.exponentialRampToValueAtTime(0.001, t + dur);
            osc.connect(gain).connect(ctx.destination);
            osc.start(t);
            osc.stop(t + dur + 0.02);
            t += dur;
        }
    } catch (_) {}
}

export function sfxMute(muted) {
    window._notepunkMuted = !!muted;
}

export function render3DGraph(containerId, nodesJson, edgesJson, onNodeTap, onEdgeTap, linkMode) {
    const container = document.getElementById(containerId);
    if (!container) return;
    if (!window.ForceGraph3D) {
        container.innerHTML = '';
        container.textContent = '// 3d-force-graph failed to load — check your connection //';
        container.style.color = '#b8442a';
        container.style.padding = '2rem';
        container.style.fontFamily = 'Courier Prime, monospace';
        return;
    }
    const rawNodes = JSON.parse(nodesJson);
    const rawEdges = JSON.parse(edgesJson);
    let maxDegree = 0;
    for (const n of rawNodes) if (n.degree > maxDegree) maxDegree = n.degree;

    const nodes = rawNodes.map(n => {
        const sl = n.skill_level || 0;
        const baseVal = 1 + Math.min(8, n.degree) + sl * 0.5;
        const labelParts = [n.label || 'untitled'];
        if (n.top_tag) labelParts.push('#' + n.top_tag);
        if (sl > 0) labelParts.push('lvl ' + sl);
        return {
            id: n.id,
            label: labelParts.join(' · '),
            degree: n.degree,
            color: _nodeColor(n.degree, n.recency, maxDegree, sl),
            val: baseVal,
            skill_level: sl,
        };
    });
    const links = rawEdges.map(e => {
        let width;
        if (e.kind === 'similarity') width = Math.max(0.4, e.weight * 3);
        else if (e.kind === 'tag') width = 1 + e.weight * 2;
        else if (e.kind === 'auto') width = 0.6;
        else width = 1.5;
        return {
            id: e.id,
            source: e.source,
            target: e.target,
            kind: e.kind,
            weight: e.weight,
            color: _edgeColor(e.kind),
            width,
            dash: _edgeDash(e.kind),
        };
    });

    _currentLinkMode = !!linkMode;

    if (_graph && _graph._container === container) {
        _graph.graphData({ nodes, links });
        return;
    }
    container.innerHTML = '';
    const g = ForceGraph3D({ controlType: 'orbit' })(container)
        .backgroundColor('#f4ead5')
        .graphData({ nodes, links })
        .nodeLabel(n => n.label || 'untitled')
        .nodeColor(n => (_highlightedNode === n.id ? '#2a4d8f' : n.color))
        .nodeVal('val')
        .nodeOpacity(0.95)
        .linkColor(l => l.color)
        .linkOpacity(l => l.kind === 'auto' ? 0.35 : 0.7)
        .linkWidth(l => l.width)
        .linkDirectionalArrowLength(l => l.kind === 'wikilink' ? 3 : 0)
        .linkDirectionalArrowRelPos(0.95)
        .linkDirectionalArrowColor(l => l.color)
        .onNodeClick(n => {
            if (_currentLinkMode) {
                if (!_pendingLink) {
                    _pendingLink = n.id;
                    _highlightedNode = n.id;
                    g.refresh();
                } else if (_pendingLink === n.id) {
                    _pendingLink = null;
                    _highlightedNode = null;
                    g.refresh();
                } else {
                    const a = _pendingLink;
                    _pendingLink = null;
                    _highlightedNode = null;
                    g.refresh();
                    onNodeTap(JSON.stringify({ action: 'link', source: a, target: n.id }));
                }
            } else {
                onNodeTap(JSON.stringify({ action: 'open', id: n.id }));
            }
        })
        .onLinkClick(l => {
            const sId = typeof l.source === 'object' ? l.source.id : l.source;
            const tId = typeof l.target === 'object' ? l.target.id : l.target;
            onEdgeTap(JSON.stringify({ id: l.id, kind: l.kind, source: sId, target: tId, weight: l.weight }));
        })
        .onBackgroundClick(() => {
            if (_pendingLink) {
                _pendingLink = null;
                _highlightedNode = null;
                g.refresh();
            }
        });
    g._container = container;
    const w = container.clientWidth || 800;
    const h = container.clientHeight || 500;
    g.width(w).height(h);
    _graph = g;

    new ResizeObserver(() => {
        if (!_graph || _graph._container !== container) return;
        const w = container.clientWidth;
        const h = container.clientHeight;
        if (w > 0 && h > 0) _graph.width(w).height(h);
    }).observe(container);
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = render3DGraph)]
    pub fn render_3d_graph(
        container_id: &str,
        nodes_json: &str,
        edges_json: &str,
        on_node_tap: &js_sys::Function,
        on_edge_tap: &js_sys::Function,
        link_mode: bool,
    );

    #[wasm_bindgen(js_name = audioStartRecording)]
    pub async fn audio_start_recording() -> JsValue;

    #[wasm_bindgen(js_name = audioStopRecording)]
    pub async fn audio_stop_recording(track_id: &str) -> JsValue;

    #[wasm_bindgen(js_name = audioPlay)]
    pub async fn audio_play(track_id: &str, looping: bool, volume: f64) -> JsValue;

    #[wasm_bindgen(js_name = audioStop)]
    pub fn audio_stop(track_id: &str) -> JsValue;

    #[wasm_bindgen(js_name = audioStopAll)]
    pub fn audio_stop_all() -> JsValue;

    #[wasm_bindgen(js_name = audioSetVolume)]
    pub fn audio_set_volume(track_id: &str, volume: f64) -> JsValue;

    #[wasm_bindgen(js_name = audioPlayMix)]
    pub async fn audio_play_mix(tracks_json: &str) -> JsValue;

    #[wasm_bindgen(js_name = audioDelete)]
    pub async fn audio_delete(track_id: &str) -> JsValue;

    #[wasm_bindgen(js_name = sfxPlay)]
    pub fn sfx_play(name: &str);

    #[wasm_bindgen(js_name = sfxMute)]
    pub fn sfx_mute(muted: bool);
}

pub fn js_field(v: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(v, &JsValue::from_str(key)).ok()
}

pub fn js_bool(v: &JsValue, key: &str) -> bool {
    js_field(v, key).and_then(|x| x.as_bool()).unwrap_or(false)
}

pub fn js_string(v: &JsValue, key: &str) -> String {
    js_field(v, key)
        .and_then(|x| x.as_string())
        .unwrap_or_default()
}

pub fn js_f64(v: &JsValue, key: &str) -> f64 {
    js_field(v, key).and_then(|x| x.as_f64()).unwrap_or(0.0)
}
