use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
let _whisperPromise = null;
let _mediaRecorder = null;
let _chunks = [];
let _stream = null;
let _cy = null;
let _pendingLink = null;

function _log(...args) { console.log('[notepunk]', ...args); }
function _err(...args) { console.error('[notepunk]', ...args); }

function _ensureWhisper() {
    if (!_whisperPromise) {
        _whisperPromise = (async () => {
            _log('loading transformers.js from CDN...');
            const mod = await import("https://cdn.jsdelivr.net/npm/@xenova/transformers@2.17.2/+esm");
            _log('library loaded; downloading Whisper-tiny.en model (~75MB on first run, then cached in IndexedDB)...');
            const p = await mod.pipeline('automatic-speech-recognition', 'Xenova/whisper-tiny.en', {
                progress_callback: (data) => {
                    if (data && data.status) _log('whisper:', data.status, data.file || '', data.progress ? Math.round(data.progress) + '%' : '');
                }
            });
            _log('whisper pipeline ready.');
            return p;
        })().catch(e => {
            _err('whisper init failed:', e);
            _whisperPromise = null;
            throw e;
        });
    }
    return _whisperPromise;
}

export async function startRecording() {
    if (_mediaRecorder && _mediaRecorder.state === 'recording') {
        return { ok: false, error: 'already recording' };
    }
    try {
        _ensureWhisper().catch(e => _err('whisper preload:', e));
        _log('requesting microphone permission...');
        _stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        _log('mic granted; starting recorder.');
        _chunks = [];
        _mediaRecorder = new MediaRecorder(_stream);
        _mediaRecorder.ondataavailable = (e) => {
            if (e.data && e.data.size > 0) _chunks.push(e.data);
        };
        _mediaRecorder.start();
        return { ok: true };
    } catch (e) {
        _err('startRecording:', e);
        return { ok: false, error: 'mic: ' + (e && e.message ? e.message : String(e)) };
    }
}

export async function stopAndTranscribe() {
    if (!_mediaRecorder || _mediaRecorder.state !== 'recording') {
        return { ok: false, error: 'not recording' };
    }
    return new Promise((resolve) => {
        _mediaRecorder.onstop = async () => {
            try {
                if (_stream) _stream.getTracks().forEach(t => t.stop());
                _log('recorder stopped; decoding audio...');
                const blob = new Blob(_chunks, { type: 'audio/webm' });
                if (blob.size === 0) {
                    resolve({ ok: false, error: 'no audio recorded' });
                    return;
                }
                const arrayBuf = await blob.arrayBuffer();
                const Ctx = window.AudioContext || window.webkitAudioContext;
                const audioCtx = new Ctx({ sampleRate: 16000 });
                const audioData = await audioCtx.decodeAudioData(arrayBuf);
                _log('audio decoded:', audioData.duration.toFixed(2) + 's @', audioData.sampleRate + 'Hz,', audioData.numberOfChannels + 'ch');
                let samples;
                if (audioData.sampleRate === 16000 && audioData.numberOfChannels === 1) {
                    samples = audioData.getChannelData(0);
                } else {
                    _log('resampling to 16kHz mono via OfflineAudioContext');
                    const offline = new OfflineAudioContext(1, Math.max(1, Math.ceil(audioData.duration * 16000)), 16000);
                    const src = offline.createBufferSource();
                    src.buffer = audioData;
                    src.connect(offline.destination);
                    src.start();
                    const resampled = await offline.startRendering();
                    samples = resampled.getChannelData(0);
                }
                _log('samples ready:', samples.length);
                _log('waiting for whisper pipeline...');
                const pipeline = await _ensureWhisper();
                _log('transcribing...');
                const result = await pipeline(samples);
                _log('transcription done:', JSON.stringify(result));
                resolve({ ok: true, text: (result.text || '').trim() });
            } catch (e) {
                _err('stopAndTranscribe:', e);
                resolve({ ok: false, error: 'transcribe: ' + (e && e.message ? e.message : String(e)) });
            }
        };
        _mediaRecorder.stop();
    });
}

function _edgeColor(kind) {
    switch (kind) {
        case 'wikilink':   return '#b8442a';
        case 'tag':        return '#2a4d8f';
        case 'similarity': return '#7a6f5c';
        case 'manual':     return '#1a1612';
        default:           return '#1a1612';
    }
}

function _edgeStyle(kind) {
    switch (kind) {
        case 'wikilink':   return 'solid';
        case 'tag':        return 'dashed';
        case 'similarity': return 'dotted';
        case 'manual':     return 'solid';
        default:           return 'solid';
    }
}

function _nodeColor(degree, recency, maxDegree) {
    const d = maxDegree > 0 ? Math.min(1, degree / maxDegree) : 0;
    const heat = 0.35 * recency + 0.65 * d;
    const r = Math.round(244 + (184 - 244) * heat);
    const g = Math.round(234 + (68  - 234) * heat);
    const b = Math.round(213 + (42  - 213) * heat);
    return 'rgb(' + r + ',' + g + ',' + b + ')';
}

export function renderGraph(containerId, nodesJson, edgesJson, onNodeTap, onEdgeTap, linkMode) {
    const container = document.getElementById(containerId);
    if (!container) return;
    if (!window.cytoscape) {
        container.innerHTML = '';
        container.textContent = '// cytoscape failed to load — check your connection //';
        container.style.color = '#b8442a';
        container.style.padding = '2rem';
        container.style.fontFamily = 'Courier Prime, monospace';
        return;
    }
    const rawNodes = JSON.parse(nodesJson);
    const rawEdges = JSON.parse(edgesJson);
    let maxDegree = 0;
    for (const n of rawNodes) if (n.degree > maxDegree) maxDegree = n.degree;

    const nodes = rawNodes.map(n => ({
        data: {
            id: n.id,
            label: n.label,
            degree: n.degree,
            color: _nodeColor(n.degree, n.recency, maxDegree),
            size: 22 + Math.min(28, n.degree * 4),
            image: n.image || '',
        }
    }));
    const edges = rawEdges.map(e => ({
        data: {
            id: e.id,
            source: e.source,
            target: e.target,
            kind: e.kind,
            weight: e.weight,
            color: _edgeColor(e.kind),
            style: _edgeStyle(e.kind),
            width: e.kind === 'similarity' ? Math.max(0.5, e.weight * 4) : (e.kind === 'tag' ? 1 + e.weight * 3 : 2),
            opacity: e.kind === 'similarity' ? Math.max(0.3, Math.min(0.95, e.weight + 0.2)) : 0.75,
        }
    }));

    container.innerHTML = '';
    _cy = window.cytoscape({
        container,
        elements: { nodes, edges },
        style: [
            { selector: 'node', style: {
                'background-color': 'data(color)',
                'background-image': 'data(image)',
                'background-fit': 'cover',
                'background-clip': 'node',
                'border-color': '#1a1612',
                'border-width': 2,
                'label': 'data(label)',
                'color': '#1a1612',
                'font-family': 'Special Elite, Courier Prime, monospace',
                'font-size': 12,
                'text-margin-y': -8,
                'text-valign': 'top',
                'text-halign': 'center',
                'width': 'data(size)',
                'height': 'data(size)',
                'shape': 'ellipse'
            }},
            { selector: 'node:selected', style: {
                'border-color': '#b8442a',
                'border-width': 4
            }},
            { selector: 'node.pending-link', style: {
                'border-color': '#2a4d8f',
                'border-width': 4,
                'border-style': 'dashed'
            }},
            { selector: 'edge', style: {
                'width': 'data(width)',
                'line-color': 'data(color)',
                'line-style': 'data(style)',
                'curve-style': 'bezier',
                'opacity': 'data(opacity)',
                'target-arrow-color': 'data(color)',
                'target-arrow-shape': 'triangle',
                'arrow-scale': 0.7
            }},
            { selector: 'edge:selected', style: {
                'width': 5,
                'opacity': 1
            }}
        ],
        layout: { name: 'cose', animate: false, padding: 30, idealEdgeLength: 130, nodeRepulsion: 8000 },
        wheelSensitivity: 0.2
    });

    _pendingLink = null;
    _cy.on('tap', 'node', (evt) => {
        const id = evt.target.id();
        if (linkMode) {
            if (!_pendingLink) {
                _pendingLink = id;
                evt.target.addClass('pending-link');
            } else if (_pendingLink === id) {
                evt.target.removeClass('pending-link');
                _pendingLink = null;
            } else {
                const a = _pendingLink;
                _pendingLink = null;
                _cy.nodes().removeClass('pending-link');
                onNodeTap(JSON.stringify({ action: 'link', source: a, target: id }));
            }
        } else {
            onNodeTap(JSON.stringify({ action: 'open', id }));
        }
    });
    _cy.on('tap', 'edge', (evt) => {
        const e = evt.target.data();
        onEdgeTap(JSON.stringify({ id: e.id, kind: e.kind, source: e.source, target: e.target, weight: e.weight }));
    });
    _cy.on('tap', (evt) => {
        if (evt.target === _cy && _pendingLink) {
            _pendingLink = null;
            _cy.nodes().removeClass('pending-link');
        }
    });
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = renderGraph)]
    pub fn render_graph(
        container_id: &str,
        nodes_json: &str,
        edges_json: &str,
        on_node_tap: &js_sys::Function,
        on_edge_tap: &js_sys::Function,
        link_mode: bool,
    );

    #[wasm_bindgen(js_name = startRecording)]
    pub async fn start_recording() -> JsValue;

    #[wasm_bindgen(js_name = stopAndTranscribe)]
    pub async fn stop_and_transcribe() -> JsValue;
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
