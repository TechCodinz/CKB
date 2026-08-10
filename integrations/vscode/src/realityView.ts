import * as vscode from 'vscode';
import { IntelligenceState, compactMemoryDigest } from './intelligence';
import { fetchRuntimeReality, type RuntimeRealityFeed } from './runtimeReality';

const VIEW_ID = 'ckb.realityView';

function nonce() {
    const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    let value = '';
    for (let i = 0; i < 32; i += 1) value += chars.charAt(Math.floor(Math.random() * chars.length));
    return value;
}

function jsonForScript(value: unknown) {
    return JSON.stringify(value ?? null)
        .replace(/</g, '\\u003c')
        .replace(/>/g, '\\u003e')
        .replace(/&/g, '\\u0026');
}

export class CkbRealityViewProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = VIEW_ID;
    private view?: vscode.WebviewView;
    private state?: IntelligenceState;
    private runtime?: RuntimeRealityFeed;
    private runtimeTimer?: ReturnType<typeof setTimeout>;
    private runtimeRefreshing = false;
    private runtimeDisposed = false;

    constructor(private readonly context: vscode.ExtensionContext) {}

    resolveWebviewView(view: vscode.WebviewView) {
        this.view = view;
        this.runtimeDisposed = false;
        view.webview.options = { enableScripts: true };
        view.webview.onDidReceiveMessage(async message => {
            switch (message?.type) {
                case 'scan': await vscode.commands.executeCommand('ckb.scan'); break;
                case 'refresh': await vscode.commands.executeCommand('ckb.deepActivity'); break;
                case 'runtimeRefresh': await this.refreshRuntimeFeed(true); break;
                case 'memory': await vscode.commands.executeCommand('ckb.queryMemory', String(message?.query || '')); break;
                case 'impact': await vscode.commands.executeCommand('ckb.impact'); break;
                case 'openNode': await vscode.commands.executeCommand('ckb.openArchitectureNode', message?.node); break;
                case 'openExplorer': await vscode.commands.executeCommand('ckb.openExplorer'); break;
                case 'intent':
                    vscode.window.setStatusBarMessage(`CKB Invisible Reality • ${String(message?.intent || 'FUSED').toUpperCase()}`, 2500);
                    break;
            }
        });
        view.onDidDispose(() => {
            this.runtimeDisposed = true;
            if (this.runtimeTimer) clearTimeout(this.runtimeTimer);
            this.runtimeTimer = undefined;
        });
        this.render();
        void this.refreshRuntimeFeed(false);
    }

    setState(state: IntelligenceState | undefined) {
        this.state = state;
        this.render();
    }

    setRuntimeFeed(runtime: RuntimeRealityFeed | undefined) {
        this.runtime = runtime;
        if (this.view) void this.view.webview.postMessage({ type: 'ckb-runtime-feed', runtime });
    }

    reveal() {
        vscode.commands.executeCommand('workbench.view.extension.ckb-reality');
    }

    private workspaceRoot() {
        return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
    }

    private scheduleRuntimePoll() {
        if (this.runtimeTimer) clearTimeout(this.runtimeTimer);
        if (this.runtimeDisposed) return;
        const config = vscode.workspace.getConfiguration('ckb');
        if (!config.get<boolean>('liveRuntimePolling', true)) return;
        const configured = Math.max(1_000, Math.min(config.get<number>('runtimePollIntervalMs', 2500), 30_000));
        const delay = this.runtime?.observed ? configured : this.runtime?.online ? Math.max(configured, 4_000) : Math.max(configured, 8_000);
        this.runtimeTimer = setTimeout(() => void this.refreshRuntimeFeed(false), delay);
    }

    private async refreshRuntimeFeed(explicit: boolean) {
        if (this.runtimeRefreshing || this.runtimeDisposed) return;
        const root = this.workspaceRoot();
        if (!root) return;
        this.runtimeRefreshing = true;
        try {
            const next = await fetchRuntimeReality(root);
            this.runtime = next;
            if (this.view) await this.view.webview.postMessage({ type: 'ckb-runtime-feed', runtime: next });
            if (explicit) {
                if (next.observed) vscode.window.setStatusBarMessage(`CKB Live Reality • ${Object.keys(next.traces).length} exact traces • ${next.runtimeNodes} runtime nodes`, 3200);
                else if (next.online) vscode.window.setStatusBarMessage('CKB Live Reality engine is online; waiting for observed application execution.', 3200);
                else vscode.window.showWarningMessage(`CKB Live Reality is not connected: ${next.error || 'runtime server unavailable'}`);
            }
        } finally {
            this.runtimeRefreshing = false;
            this.scheduleRuntimePoll();
        }
    }

    private render() {
        if (!this.view) return;
        const webview = this.view.webview;
        const token = nonce();
        const payload = { state: this.state, digest: compactMemoryDigest(this.state), runtime: this.runtime };
        webview.html = `<!doctype html>
<html lang="en"><head><meta charset="UTF-8"/><meta name="viewport" content="width=device-width,initial-scale=1"/>
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'nonce-${token}'; script-src 'nonce-${token}';"/>
<style nonce="${token}">
:root{color-scheme:dark;--bg:#030711;--panel:#08111f;--line:#183247;--cyan:#49e8ff;--green:#58efa9;--violet:#c99cff;--amber:#ffc46f;--red:#ff758e;--muted:#7e91ab;--text:#eff9ff}*{box-sizing:border-box}body{margin:0;background:radial-gradient(circle at 50% -10%,#0b293666,transparent 36%),linear-gradient(#030711,#040914);color:var(--text);font:12px/1.45 var(--vscode-font-family,system-ui)}.shell{padding:10px}.hero,.panel{border:1px solid var(--line);background:linear-gradient(145deg,#091322f2,#050a13f5);border-radius:14px}.hero{padding:13px;position:relative;overflow:hidden}.hero:after{content:'';position:absolute;inset:0;pointer-events:none;background-image:linear-gradient(#49e8ff0c 1px,transparent 1px),linear-gradient(90deg,#49e8ff0a 1px,transparent 1px);background-size:24px 24px;mask-image:linear-gradient(#0008,transparent)}.row{display:flex;align-items:center;gap:6px;flex-wrap:wrap}.grow{flex:1}.eyebrow{color:var(--cyan);font-size:9px;font-weight:900;letter-spacing:1.5px}.dot{width:7px;height:7px;border-radius:50%;background:var(--cyan);box-shadow:0 0 12px var(--cyan)}.dot.live{background:var(--green);box-shadow:0 0 14px var(--green)}h1{font-size:17px;margin:6px 0 2px}.sub{color:var(--muted);font-size:10px}.badges{display:flex;gap:5px;flex-wrap:wrap;margin-top:9px}.badge{padding:2px 6px;border-radius:999px;border:1px solid #ffffff18;font-size:8px;font-weight:850}.static{color:var(--cyan)}.runtime{color:var(--green)}.predicted{color:var(--violet)}.fault{color:var(--red)}.toolbar{display:grid;grid-template-columns:repeat(2,1fr);gap:6px;margin-top:10px}button{border:1px solid #ffffff18;background:#ffffff08;color:var(--text);border-radius:8px;padding:7px 8px;font:600 10px var(--vscode-font-family,system-ui);cursor:pointer}button:hover{border-color:#49e8ff66;background:#49e8ff0b}button:disabled{opacity:.38;cursor:not-allowed}.primary{border-color:#49e8ff55;background:linear-gradient(100deg,#49e8ff18,#c99cff10)}.section{margin-top:10px}.title{font-size:9px;font-weight:900;letter-spacing:1.1px;color:#b8c8da;margin:0 2px 6px}.metrics{display:grid;grid-template-columns:repeat(2,1fr);gap:6px}.metric{padding:8px;border:1px solid #ffffff12;background:#09111ddd;border-radius:10px}.metric strong{display:block;font-size:16px}.metric span{font-size:8px;color:var(--muted)}.lensbar,.depthbar,.flowbar{display:flex;gap:5px;overflow:auto;padding-bottom:3px}.chipbtn{white-space:nowrap;padding:5px 7px;font-size:8px;border-radius:999px}.chipbtn.active{background:#49e8ff18;border-color:#49e8ff77;color:var(--cyan)}.panel{padding:9px}.scope{color:#b8c6d7;font-size:10px}.scope b{color:var(--cyan)}.cards{margin-top:7px}.card{padding:8px;border:1px solid #ffffff11;border-radius:9px;background:#ffffff05;margin-top:5px;cursor:pointer}.card:hover{border-color:#49e8ff55}.name{font-weight:800;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.path{font:8px var(--vscode-editor-font-family,monospace);color:#667c99;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.meta{display:flex;gap:5px;flex-wrap:wrap;margin-top:5px;color:#8ea1b8;font-size:8px}.meter{height:3px;background:#ffffff0d;border-radius:9px;margin-top:6px;overflow:hidden}.meter i{display:block;height:100%;background:linear-gradient(90deg,var(--cyan),var(--violet),var(--amber))}.empty{padding:16px;text-align:center;color:var(--muted);border:1px dashed #ffffff18;border-radius:9px}.intent{display:grid;grid-template-columns:repeat(3,1fr);gap:5px}.intent button{font-size:8px;padding:5px}.intent button.active{border-color:#c99cff77;color:var(--violet);background:#c99cff11}.memory{display:flex;gap:5px}.memory input{min-width:0;flex:1;border:1px solid #ffffff1a;background:#02050b99;color:var(--text);padding:7px;border-radius:8px;font:10px var(--vscode-font-family,system-ui)}.note{margin-top:7px;color:var(--muted);font-size:8px}.error{color:#ffd19a;border:1px solid #ffc46f44;background:#ffc46f0a;padding:7px;border-radius:8px;margin-top:7px}.runtimepanel{position:relative;overflow:hidden}.runtimepanel:before{content:'';position:absolute;inset:-40% 20% auto -20%;height:120px;background:radial-gradient(circle,#58efa922,transparent 70%);pointer-events:none}.trace{padding:7px;border-left:2px solid var(--green);background:#58efa907;margin-top:5px;border-radius:0 8px 8px 0;cursor:pointer}.trace.errorTrace{border-left-color:var(--red);background:#ff758e08}.trace code{font:8px var(--vscode-editor-font-family,monospace);word-break:break-all;color:#c9f7dd}.trace small{display:block;color:var(--muted);margin-top:3px}.liveHead{display:flex;align-items:center;gap:6px}.liveHead strong{font-size:10px}
</style></head><body><div class="shell">
<section class="hero"><div class="row"><span class="dot"></span><span class="eyebrow">CKB INVISIBLE REALITY V8</span><span class="grow"></span><span class="badge static">TRUTH-GUARDED</span></div><h1>Molecular Software Microscope</h1><div class="sub">Architecture memory plus exact runtime transmission evidence inside the editor.</div><div class="badges"><span class="badge static">STATIC</span><span class="badge runtime">RUNTIME</span><span class="badge predicted">PREDICTED</span></div><div class="toolbar"><button class="primary" data-action="refresh">◉ Deep Analyze</button><button data-action="impact">⌁ Cursor Ripple</button><button data-action="scan">↻ Base Scan</button><button data-action="openExplorer">↗ Cloud Universe</button></div></section>
<section class="section"><div class="title">SYSTEM REALITY</div><div class="metrics" id="metrics"></div></section>
<section class="section panel runtimepanel"><div class="liveHead"><span id="runtimeDot" class="dot"></span><strong>LIVE TRANSMISSION FIELD V8</strong><span class="grow"></span><button data-action="runtimeRefresh">↻ LIVE</button></div><div class="note" id="runtimeState"></div><div class="flowbar" id="flowbar"></div><div id="runtimeBody"></div></section>
<section class="section panel"><div class="title">HYBRID INTELLIGENT INTENT</div><div class="intent" id="intents"></div><div class="note" id="intentNote"></div></section>
<section class="section panel"><div class="title">INVISIBLE REALITY LENS</div><div class="lensbar" id="lenses"></div><div class="title" style="margin-top:8px">SEMANTIC DEPTH</div><div class="depthbar" id="depths"></div><div class="scope" id="scope"></div><div class="cards" id="cards"></div></section>
<section class="section panel"><div class="title">ARCHITECTURE MEMORY</div><div class="memory"><input id="memoryQuery" placeholder="Ask about flow, risk, ownership or a symbol"/><button data-action="memory">Ask</button></div><div class="note">Queries use bounded architecture memory. CKB does not fabricate runtime evidence when telemetry is absent.</div></section>
<div id="error"></div></div>
<script nonce="${token}">
const vscode=acquireVsCodeApi();const payload=${jsonForScript(payload)};const state=payload.state||{};let runtime=payload.runtime||{};const activity=state.activity||state.bundle?.activity||{};const dna=state.dna||state.bundle?.dna||{};const hotspots=Array.isArray(activity.hotspots)?activity.hotspots:[];const runtimeNodes=hotspots.filter(n=>n&&n.runtimeObserved);const faults=hotspots.filter(n=>Number(n?.errorRate||n?.runtime?.errorRate||0)>0);let lens='semantic',depth='system',intent='auto',flowFilter='all';
const esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));const num=v=>Number.isFinite(Number(v))?Number(v).toLocaleString():'—';const pct=v=>Number.isFinite(Number(v))?Math.round(Number(v)*100)+'%':'—';
const metrics=[['SYMBOLS',activity.nodesAnalyzed??state.scan?.nodes],['RELATIONS',activity.edgesAnalyzed??state.scan?.edges],['RUNTIME',Number.isFinite(Number(activity.runtimeCoveragePct))?Number(activity.runtimeCoveragePct).toFixed(1)+'%':'—'],['CODE DNA',Number.isFinite(Number(dna.overallHealth))?Number(dna.overallHealth).toFixed(1)+'%':'—']];document.getElementById('metrics').innerHTML=metrics.map(x=>'<div class="metric"><strong>'+esc(x[1]??'—')+'</strong><span>'+x[0]+'</span></div>').join('');
const intents=['auto','fused','live','fault','change','memory'];const lenses=['semantic','molecule','nanotrace','state'];const depths=['system','subsystem','file','symbol','call','runtime'];const flowTypes=['http','database','cache','queue','event','websocket','function','other'];
function resolvedIntent(){if(intent!=='auto')return intent;if(faults.length)return'fault';if(runtimeNodes.length||runtime.observed)return'live';return'fused'}
function allTraceSteps(){const rows=[];Object.entries(runtime.traces||{}).forEach(([traceId,steps])=>(Array.isArray(steps)?steps:[]).forEach((step,index)=>rows.push({...step,traceId:step.traceId||traceId,__index:index})));return rows}
function flowType(step){const text=(String(step.flowType||'')+' '+String(step.operation||'')+' '+String(step.protocol||'')+' '+String(step.dbSystem||'')+' '+String(step.messagingSystem||'')).toLowerCase();if(/websocket|\\bws\\b|\\bwss\\b/.test(text))return'websocket';if(/redis|cache/.test(text))return'cache';if(/postgres|mysql|sqlite|mongo|prisma|database|\\bsql\\b/.test(text))return'database';if(/queue|kafka|rabbit|bull|sqs|pubsub|message/.test(text))return'queue';if(/event/.test(text))return'event';if(/http|rpc|fetch|request|response/.test(text))return'http';if(/function|call|internal|handler|method/.test(text))return'function';return'other'}
function renderRuntime(){const stateEl=document.getElementById('runtimeState');const dot=document.getElementById('runtimeDot');dot.className='dot'+(runtime.observed?' live':'');const steps=allTraceSteps();stateEl.textContent=runtime.observed?(runtime.replaySafe?'EXACT OBSERVED TRACES • '+num(Object.keys(runtime.traces||{}).length)+' traces • '+num(runtime.runtimeNodes||0)+' runtime nodes':'RUNTIME OBSERVED • exact parent/child retrace not available yet'):(runtime.online?'ENGINE ONLINE • waiting for observed execution':'RUNTIME SERVER OFFLINE • static architecture remains available');const counts=runtime.flowCounts||{};document.getElementById('flowbar').innerHTML=['all',...flowTypes].map(type=>'<button class="chipbtn '+(flowFilter===type?'active':'')+'" data-flow="'+type+'">'+type.toUpperCase()+(type==='all'?'':' '+num(counts[type]||0))+'</button>').join('');let filtered=flowFilter==='all'?steps:steps.filter(s=>flowType(s)===flowFilter);filtered=filtered.slice(-18).reverse();document.getElementById('runtimeBody').innerHTML=filtered.length?filtered.map(s=>'<div class="trace '+(s.error?'errorTrace':'')+'" data-runtime-node="'+esc(encodeURIComponent(JSON.stringify({id:s.target||s.source,path:String(s.target||s.source||'').split('::')[0],name:String(s.target||s.source||'').split('::').pop()})))+'"><div class="row"><b>'+esc(flowType(s).toUpperCase())+'</b><span class="grow"></span><span class="badge '+(s.error?'fault':'runtime')+'">'+(s.error?'ERROR':'OBSERVED')+'</span></div><code>'+esc(s.source||'unknown')+' → '+esc(s.target||'unknown')+'</code><small>'+esc(s.operation||'runtime transition')+' • '+(Number.isFinite(Number(s.durationMs))?Number(s.durationMs).toFixed(2)+' ms':'duration n/a')+'</small></div>').join(''):'<div class="empty">No exact runtime transmission matches this filter. Run the project with CKB Live Reality attached; static dependencies are never animated as execution.</div>';bindRuntime()}
function renderControls(){document.getElementById('intents').innerHTML=intents.map(x=>'<button class="'+(intent===x?'active':'')+'" data-intent="'+x+'" '+((x==='live'&&!runtimeNodes.length&&!runtime.observed)?'disabled':'')+'>'+x.toUpperCase()+'</button>').join('');document.getElementById('lenses').innerHTML=lenses.map(x=>'<button class="chipbtn '+(lens===x?'active':'')+'" data-lens="'+x+'" '+((x==='nanotrace'&&!runtime.replaySafe)?'disabled':'')+'>'+x.toUpperCase()+'</button>').join('');document.getElementById('depths').innerHTML=depths.map(x=>'<button class="chipbtn '+(depth===x?'active':'')+'" data-depth="'+x+'" '+((x==='runtime'&&!runtime.observed&&!runtimeNodes.length)?'disabled':'')+'>'+x.toUpperCase()+'</button>').join('');document.getElementById('intentNote').textContent='AUTO resolved to '+resolvedIntent().toUpperCase()+' from available evidence.';bindControls()}
function selectedItems(){let list=hotspots.slice();if(lens==='nanotrace'||depth==='runtime'||resolvedIntent()==='live')list=list.filter(n=>n.runtimeObserved);if(lens==='state'||resolvedIntent()==='fault')list=list.filter(n=>Number(n?.errorRate||n?.runtime?.errorRate||0)>0||Number(n?.changeSensitivityIndex||0)>.45);if(depth==='system')return list.slice(0,12);if(depth==='subsystem')return list.slice(0,16);if(depth==='file')return list.filter(n=>n.path).slice(0,18);if(depth==='call')return list.filter(n=>Number(n.fanIn||0)+Number(n.fanOut||0)>0).slice(0,18);return list.slice(0,18)}
function renderReality(){const list=selectedItems();document.getElementById('scope').innerHTML='<b>'+esc(lens.toUpperCase())+'</b> • '+esc(depth.toUpperCase())+' • '+esc(resolvedIntent().toUpperCase())+' • '+num(list.length)+' visible evidence-backed symbols';document.getElementById('cards').innerHTML=list.length?list.map(n=>{const score=Math.max(Number(n.activityIndex||0),Number(n.changeSensitivityIndex||0));return '<div class="card" data-node="'+esc(encodeURIComponent(JSON.stringify({id:n.id,name:n.name,path:n.path,line:n.line,column:n.column})))+'"><div class="row"><div class="grow"><div class="name">'+esc(n.name||n.id||'symbol')+'</div><div class="path">'+esc(n.path||n.id||'')+'</div></div><span class="badge '+(n.runtimeObserved?'runtime':'static')+'">'+(n.runtimeObserved?'RUNTIME':'STATIC')+'</span></div><div class="meta"><span>'+esc(n.role||'architecture symbol')+'</span><span>in '+num(n.fanIn)+'</span><span>out '+num(n.fanOut)+'</span><span>change '+pct(n.changeSensitivityIndex)+'</span></div><div class="meter"><i style="width:'+Math.max(2,Math.min(100,score*100))+'%"></i></div></div>'}).join(''):'<div class="empty">No evidence-backed symbols match this reality lens. Attach telemetry for runtime/nanotrace views.</div>';bindNodes()}
function bindControls(){document.querySelectorAll('[data-intent]').forEach(el=>el.onclick=()=>{intent=el.dataset.intent;vscode.postMessage({type:'intent',intent:resolvedIntent()});renderControls();renderReality()});document.querySelectorAll('[data-lens]').forEach(el=>el.onclick=()=>{lens=el.dataset.lens;renderControls();renderReality()});document.querySelectorAll('[data-depth]').forEach(el=>el.onclick=()=>{depth=el.dataset.depth;renderControls();renderReality()});}
function bindNodes(){document.querySelectorAll('[data-node]').forEach(el=>el.onclick=()=>{try{vscode.postMessage({type:'openNode',node:JSON.parse(decodeURIComponent(el.dataset.node))})}catch{}})}
function bindRuntime(){document.querySelectorAll('[data-flow]').forEach(el=>el.onclick=()=>{flowFilter=el.dataset.flow;renderRuntime()});document.querySelectorAll('[data-runtime-node]').forEach(el=>el.onclick=()=>{try{vscode.postMessage({type:'openNode',node:JSON.parse(decodeURIComponent(el.dataset.runtimeNode))})}catch{}})}
document.querySelectorAll('[data-action]').forEach(el=>el.onclick=()=>{const action=el.dataset.action;if(action==='memory'){vscode.postMessage({type:'memory',query:document.getElementById('memoryQuery').value||''})}else vscode.postMessage({type:action})});window.addEventListener('message',event=>{const message=event.data||{};if(message.type!=='ckb-runtime-feed')return;runtime=message.runtime||{};renderRuntime();renderControls();renderReality()});renderRuntime();renderControls();renderReality();if(state.error)document.getElementById('error').innerHTML='<div class="error">'+esc(state.error)+'</div>';
</script></body></html>`;
    }
}
