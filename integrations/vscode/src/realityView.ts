import * as vscode from 'vscode';
import { IntelligenceState, compactMemoryDigest } from './intelligence';

const VIEW_ID = 'ckb.realityView';

function nonce() {
    const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    let value = '';
    for (let i = 0; i < 32; i += 1) value += chars.charAt(Math.floor(Math.random() * chars.length));
    return value;
}

function jsonForScript(value: any) {
    return JSON.stringify(value ?? null).replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026');
}

export class CkbRealityViewProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = VIEW_ID;
    private view?: vscode.WebviewView;
    private state?: IntelligenceState;

    constructor(private readonly context: vscode.ExtensionContext) {}

    resolveWebviewView(view: vscode.WebviewView) {
        this.view = view;
        view.webview.options = { enableScripts: true };
        view.webview.onDidReceiveMessage(async message => {
            switch (message?.type) {
                case 'scan':
                    await vscode.commands.executeCommand('ckb.scan');
                    break;
                case 'refresh':
                    await vscode.commands.executeCommand('ckb.deepActivity');
                    break;
                case 'memory':
                    await vscode.commands.executeCommand('ckb.queryMemory', String(message?.query || ''));
                    break;
                case 'impact':
                    await vscode.commands.executeCommand('ckb.impact');
                    break;
                case 'openNode':
                    await vscode.commands.executeCommand('ckb.openArchitectureNode', message?.node);
                    break;
                case 'openExplorer':
                    await vscode.commands.executeCommand('ckb.openExplorer');
                    break;
            }
        });
        this.render();
    }

    setState(state: IntelligenceState | undefined) {
        this.state = state;
        this.render();
    }

    reveal() {
        vscode.commands.executeCommand('workbench.view.extension.ckb-reality');
    }

    private render() {
        if (!this.view) return;
        const webview = this.view.webview;
        const token = nonce();
        const state = this.state;
        const payload = {
            state,
            digest: compactMemoryDigest(state),
        };
        webview.html = `<!doctype html>
<html lang="en">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=device-width, initial-scale=1.0" />
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} data:; style-src 'nonce-${token}'; script-src 'nonce-${token}';" />
<style nonce="${token}">
:root{color-scheme:dark;--bg:#05070d;--panel:rgba(11,16,29,.86);--line:rgba(67,233,255,.18);--cyan:#43e9ff;--green:#50f2a4;--purple:#c28cff;--amber:#ffbd66;--red:#ff617b;--muted:#8090aa;--text:#eef7ff}
*{box-sizing:border-box}body{margin:0;background:radial-gradient(circle at 50% -20%,rgba(67,233,255,.12),transparent 35%),radial-gradient(circle at 100% 25%,rgba(194,140,255,.09),transparent 32%),var(--bg);color:var(--text);font:12px/1.45 var(--vscode-font-family,system-ui);overflow-x:hidden}.shell{padding:10px 10px 18px}.hero{position:relative;overflow:hidden;border:1px solid var(--line);border-radius:14px;padding:14px;background:linear-gradient(145deg,rgba(12,20,35,.97),rgba(7,10,19,.94));box-shadow:inset 0 0 34px rgba(67,233,255,.035),0 12px 32px rgba(0,0,0,.24)}.hero:after{content:"";position:absolute;inset:auto -35% -85% 20%;height:150px;background:radial-gradient(ellipse,rgba(67,233,255,.2),transparent 65%);pointer-events:none}.eyebrow{display:flex;align-items:center;gap:7px;color:var(--cyan);font-size:10px;font-weight:900;letter-spacing:1.5px}.pulse{width:7px;height:7px;border-radius:50%;background:var(--cyan);box-shadow:0 0 13px var(--cyan)}h1{font-size:17px;line-height:1.1;margin:7px 0 5px;letter-spacing:-.35px}.sub{color:var(--muted);font-size:10.5px}.truth{display:flex;gap:5px;flex-wrap:wrap;margin-top:10px}.badge{padding:3px 7px;border-radius:999px;border:1px solid rgba(255,255,255,.1);font-size:9px;font-weight:800;letter-spacing:.6px}.static{color:var(--cyan);border-color:rgba(67,233,255,.3)}.runtime{color:var(--green);border-color:rgba(80,242,164,.3)}.predicted{color:var(--purple);border-color:rgba(194,140,255,.3)}.toolbar{display:grid;grid-template-columns:repeat(2,1fr);gap:6px;margin-top:10px}button{appearance:none;border:1px solid rgba(255,255,255,.11);background:rgba(255,255,255,.035);color:var(--text);border-radius:8px;padding:7px 8px;font:600 10.5px var(--vscode-font-family,system-ui);cursor:pointer;text-align:left}button:hover{border-color:rgba(67,233,255,.45);background:rgba(67,233,255,.06)}button.primary{background:linear-gradient(100deg,rgba(67,233,255,.14),rgba(194,140,255,.11));border-color:rgba(67,233,255,.32);color:#dffcff}.section{margin-top:12px}.section-title{display:flex;justify-content:space-between;align-items:center;color:#b9c8dc;font-weight:900;font-size:9.5px;letter-spacing:1.15px;text-transform:uppercase;margin:0 2px 6px}.grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:6px}.metric{border:1px solid rgba(255,255,255,.075);border-radius:10px;padding:9px;background:var(--panel);min-width:0}.metric strong{display:block;font-size:16px;line-height:1.1;color:var(--text);font-weight:850}.metric span{display:block;margin-top:4px;color:var(--muted);font-size:9px}.metric.cyan strong{color:var(--cyan)}.metric.green strong{color:var(--green)}.metric.purple strong{color:var(--purple)}.metric.amber strong{color:var(--amber)}.tabs{display:flex;gap:5px;overflow:auto;padding:1px 1px 4px;scrollbar-width:none}.tabs::-webkit-scrollbar{display:none}.tab{flex:0 0 auto;padding:6px 9px;font-size:9.5px;border-radius:999px}.tab.active{color:var(--cyan);border-color:rgba(67,233,255,.45);background:rgba(67,233,255,.08)}.pane{display:none}.pane.active{display:block}.card{border:1px solid rgba(255,255,255,.075);background:linear-gradient(145deg,rgba(13,18,31,.94),rgba(7,10,18,.9));border-radius:11px;padding:9px;margin-bottom:6px;cursor:pointer}.card:hover{border-color:rgba(67,233,255,.3)}.row{display:flex;align-items:center;gap:6px;min-width:0}.grow{flex:1;min-width:0}.name{font-weight:800;color:#eaf4ff;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.path{font:9px/1.3 var(--vscode-editor-font-family,monospace);color:#6f819e;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;margin-top:2px}.mini{font-size:8.5px;padding:2px 5px}.meter{height:3px;border-radius:99px;background:rgba(255,255,255,.06);overflow:hidden;margin-top:7px}.meter i{display:block;height:100%;background:linear-gradient(90deg,var(--cyan),var(--purple));box-shadow:0 0 8px rgba(67,233,255,.5)}.meta{display:flex;gap:5px;flex-wrap:wrap;margin-top:6px;color:#91a2ba;font-size:8.5px}.memory-box{border:1px solid rgba(194,140,255,.18);border-radius:11px;padding:9px;background:rgba(194,140,255,.035)}.memory-search{display:flex;gap:5px}.memory-search input{min-width:0;flex:1;border:1px solid rgba(255,255,255,.12);outline:none;border-radius:8px;background:rgba(0,0,0,.25);color:var(--text);padding:7px 8px;font:10px var(--vscode-font-family,system-ui)}.memory-search input:focus{border-color:rgba(194,140,255,.55)}pre{max-height:240px;overflow:auto;white-space:pre-wrap;word-break:break-word;font:9px/1.45 var(--vscode-editor-font-family,monospace);color:#aebed2;background:rgba(0,0,0,.18);padding:8px;border-radius:7px;margin:7px 0 0}.empty{padding:18px 12px;text-align:center;color:var(--muted);border:1px dashed rgba(255,255,255,.1);border-radius:10px}.error{margin-top:8px;padding:7px 9px;border:1px solid rgba(255,189,102,.24);background:rgba(255,189,102,.055);border-radius:8px;color:#d9b680;font-size:9px}.source{font:8.5px var(--vscode-editor-font-family,monospace);color:#63748f;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.orbital{height:76px;position:relative;margin:7px 0 0;opacity:.82}.ring{position:absolute;border:1px solid rgba(67,233,255,.16);border-radius:50%;left:50%;top:50%;transform:translate(-50%,-50%)}.r1{width:68px;height:68px}.r2{width:45px;height:45px;border-color:rgba(194,140,255,.18)}.core{position:absolute;width:9px;height:9px;border-radius:50%;background:var(--cyan);box-shadow:0 0 17px var(--cyan);left:calc(50% - 4px);top:calc(50% - 4px)}.sat{position:absolute;width:5px;height:5px;border-radius:50%;background:var(--green);box-shadow:0 0 8px var(--green)}.s1{left:23%;top:29%}.s2{right:22%;top:35%;background:var(--purple);box-shadow:0 0 8px var(--purple)}.s3{left:34%;bottom:15%;background:var(--amber);box-shadow:0 0 8px var(--amber)}@media(prefers-reduced-motion:no-preference){.pulse,.core{animation:breathe 2.1s ease-in-out infinite}@keyframes breathe{50%{opacity:.55;transform:scale(.75)}}}
</style>
</head>
<body>
<div class="shell">
  <section class="hero">
    <div class="eyebrow"><span class="pulse"></span> CKB LIVING REALITY</div>
    <h1>Architecture Intelligence Core</h1>
    <div class="sub">See structure, observed execution, change pressure and model memory as one evidence-separated system.</div>
    <div class="truth"><span class="badge static">STATIC</span><span class="badge runtime">RUNTIME</span><span class="badge predicted">PREDICTED</span></div>
    <div class="orbital"><div class="ring r1"></div><div class="ring r2"></div><div class="core"></div><div class="sat s1"></div><div class="sat s2"></div><div class="sat s3"></div></div>
    <div class="toolbar"><button class="primary" data-action="refresh">◉ Deep Analyze</button><button data-action="impact">⌁ Cursor Impact</button><button data-action="scan">↻ Base Scan</button><button data-action="openExplorer">↗ Cloud Explorer</button></div>
  </section>
  <section class="section">
    <div class="section-title"><span>System Reality</span><span id="source" class="source"></span></div>
    <div class="grid" id="metrics"></div>
  </section>
  <section class="section">
    <div class="tabs"><button class="tab active" data-tab="activity">Activity</button><button class="tab" data-tab="change">Change Pressure</button><button class="tab" data-tab="boundaries">Boundaries</button><button class="tab" data-tab="memory">Memory</button></div>
    <div id="activity" class="pane active"></div>
    <div id="change" class="pane"></div>
    <div id="boundaries" class="pane"></div>
    <div id="memory" class="pane"></div>
  </section>
  <div id="error"></div>
</div>
<script nonce="${token}">
const vscode=acquireVsCodeApi();const payload=${jsonForScript(payload)};const state=payload.state||{};const activity=state.activity||state.bundle?.activity||{};const dna=state.dna||state.bundle?.dna||{};const memory=state.memory||state.bundle?.memory||{};const digest=payload.digest||{};
const esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));const pct=v=>Number.isFinite(Number(v))?(Number(v)*100).toFixed(0)+'%':'—';const rawPct=v=>Number.isFinite(Number(v))?Number(v).toFixed(1)+'%':'—';const num=v=>Number.isFinite(Number(v))?Number(v).toLocaleString():'—';
document.getElementById('source').textContent=state.source||'awaiting analysis';const metrics=[['Symbols',activity.nodesAnalyzed??state.scan?.nodes,'cyan'],['Relations',activity.edgesAnalyzed??state.scan?.edges,'purple'],['Runtime coverage',Number.isFinite(Number(activity.runtimeCoveragePct))?rawPct(activity.runtimeCoveragePct):'—','green'],['Code DNA',Number.isFinite(Number(dna.overallHealth))?Number(dna.overallHealth).toFixed(1)+'%':'—','amber']];document.getElementById('metrics').innerHTML=metrics.map(x=>'<div class="metric '+x[2]+'"><strong>'+esc(x[1]??'—')+'</strong><span>'+esc(x[0])+'</span></div>').join('');
function nodeCard(n,indexField){const idx=Number(n?.[indexField]??n?.activityIndex??0);const runtime=n?.runtimeObserved?'<span class="badge mini runtime">RUNTIME</span>':'<span class="badge mini static">STATIC</span>';return '<div class="card" data-node="'+esc(encodeURIComponent(JSON.stringify({id:n?.id,path:n?.path,line:n?.line,column:n?.column,name:n?.name})))+'"><div class="row"><div class="grow"><div class="name">'+esc(n?.name||n?.id||'symbol')+'</div><div class="path">'+esc(n?.path||n?.id||'')+'</div></div>'+runtime+'</div><div class="meta"><span>'+esc(n?.role||'architecture symbol')+'</span><span>fan-in '+num(n?.fanIn)+'</span><span>fan-out '+num(n?.fanOut)+'</span></div><div class="meter"><i style="width:'+Math.max(1,Math.min(100,idx*100))+'%"></i></div></div>'}
const hot=Array.isArray(activity.hotspots)?activity.hotspots.slice(0,18):[];document.getElementById('activity').innerHTML=hot.length?hot.map(n=>nodeCard(n,'activityIndex')).join(''):'<div class="empty">Run Deep Analyze to build the local architecture activity field.</div>';
const change=Array.isArray(activity.changeSensitive)?activity.changeSensitive.slice(0,18):[];document.getElementById('change').innerHTML=change.length?change.map(n=>nodeCard(n,'changeSensitivityIndex')).join(''):'<div class="empty">No change-sensitivity model is hydrated yet.</div>';
const boundaries=Array.isArray(activity.boundaries)?activity.boundaries.slice(0,20):[];document.getElementById('boundaries').innerHTML=boundaries.length?boundaries.map(b=>'<div class="card"><div class="row"><div class="grow"><div class="name">'+esc(b.id)+'</div><div class="path">'+esc(b.kind||'boundary')+'</div></div><span class="badge mini static">STATIC</span></div><div class="meta"><span>'+num(b.symbols)+' symbols</span><span>'+num(b.incomingCrossBoundary)+' incoming</span><span>'+num(b.outgoingCrossBoundary)+' outgoing</span><span>'+num(b.runtimeObservedSymbols)+' runtime-observed</span></div><div class="meter"><i style="width:'+Math.max(1,Math.min(100,Number(b.activityIndex||0)*100))+'%"></i></div></div>').join(''):'<div class="empty">Boundary activity appears after deep analysis.</div>';
const memoryContext=memory?.context||state.bundle?.memory?.context||'';document.getElementById('memory').innerHTML='<div class="memory-box"><div class="memory-search"><input id="memoryQuery" aria-label="Architecture memory query" placeholder="Ask the architecture memory…"/><button id="memoryGo">Query</button></div><div class="meta"><span>'+num(memory?.retrieval?.retrievedNodes)+' retrieved nodes</span><span>'+num(memory?.retrieval?.runtimeObservedNodes)+' runtime-observed</span><span>'+esc(memory?.retrieval?.truncated?'bounded slice':'complete slice')+'</span></div>'+(memoryContext?'<pre>'+esc(memoryContext.slice(0,14000))+'</pre>':'<div class="empty" style="margin-top:8px">Query symbols, services, flows, risks or responsibilities without dumping the whole repository into model context.</div>')+'</div>';
if(state.error)document.getElementById('error').innerHTML='<div class="error">'+esc(state.error)+'</div>';document.querySelectorAll('.tab').forEach(btn=>btn.addEventListener('click',()=>{document.querySelectorAll('.tab').forEach(x=>x.classList.remove('active'));document.querySelectorAll('.pane').forEach(x=>x.classList.remove('active'));btn.classList.add('active');document.getElementById(btn.dataset.tab).classList.add('active')}));document.querySelectorAll('[data-action]').forEach(btn=>btn.addEventListener('click',()=>vscode.postMessage({type:btn.dataset.action})));document.querySelectorAll('[data-node]').forEach(card=>card.addEventListener('click',()=>{try{vscode.postMessage({type:'openNode',node:JSON.parse(decodeURIComponent(card.dataset.node))})}catch{}}));const go=()=>{const input=document.getElementById('memoryQuery');const query=String(input?.value||'').trim();if(query)vscode.postMessage({type:'memory',query})};document.getElementById('memoryGo')?.addEventListener('click',go);document.getElementById('memoryQuery')?.addEventListener('keydown',e=>{if(e.key==='Enter')go()});
</script>
</body></html>`;
    }
}
