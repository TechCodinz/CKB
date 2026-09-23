#!/usr/bin/env python3
"""Stage CKB Rust services and OmniCode on the existing VPS; no public cutover."""
import base64
import getpass
import json
import os
import re
import shutil
import socket
import subprocess
import time
import warnings
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.parse import urlencode, quote
from urllib.error import HTTPError

BASE = Path('/opt/app-platform')
TEAM = 'team_cqD6MhAvwjS8MkdD4Vvw76sG'
PROJECT = 'prj_z92XVKjAKFbxOoMfA3p9Unwt6PeD'
CKB_REF = 'dce79ffcb9f01eb991ba1bd13572edcb75aa9032'
OMNI_REF = '6869aa0117ba47fcc5d0f53b4e27cdbad15259a5'

class Stop(Exception):
    pass

def compose_env(values):
    return {k: v.replace('$', '$$') for k, v in values.items()}

def production_values(rows, retrieve=None):
    result = {}
    unavailable = []
    seen = set()
    for row in rows:
        target = row.get('target', [])
        if 'production' not in ([target] if isinstance(target, str) else target):
            continue
        key = row['key']
        if not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', key):
            raise Stop('Invalid environment variable name')
        if key in seen:
            raise Stop('Duplicate production setting: ' + key)
        seen.add(key)
        if row.get('type') == 'sensitive':
            unavailable.append(key + ' (non-exportable sensitive value)')
            continue
        readable = isinstance(row.get('value'), str) and (
            row.get('type') in ('plain', 'system') or row.get('decrypted') is True
        )
        if not readable and retrieve is not None and row.get('id'):
            # The list endpoint's bulk decrypt option is deprecated. Fetch the
            # authorized plaintext through the dedicated per-variable endpoint.
            try:
                item = retrieve(row['id'])
            except Stop as error:
                unavailable.append(key + ' (' + str(error) + ')')
                continue
            if not isinstance(item, dict) or item.get('key') != key or (
                item.get('id') is not None and item['id'] != row['id']
            ):
                raise Stop('Vercel environment identity mismatch: ' + key)
            readable = (item.get('type') != 'sensitive'
                        and item.get('decrypted') is True
                        and isinstance(item.get('value'), str))
            if readable:
                row = item
        if not readable:
            unavailable.append(key + ' (no verified plaintext returned)')
            continue
        result[key] = row['value']
    if unavailable:
        raise Stop('Vercel could not export these production settings: ' + ', '.join(unavailable))
    return result

def get_json(url, token):
    try:
        req = Request(url, headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/json'})
        with urlopen(req, timeout=30) as response:
            return json.load(response)
    except HTTPError as e:
        raise Stop('Provider request failed: HTTP ' + str(e.code)) from None

def render_values(service, token):
    values, cursor, seen = {}, '', set()
    while True:
        page = get_json('https://api.render.com/v1/services/' + service + '/env-vars?' +
                        urlencode({'limit':100, 'cursor':cursor}), token)
        if not isinstance(page, list):
            raise Stop('Unexpected Render environment response')
        for item in page:
            entry = item['envVar']
            key, value = entry['key'], entry['value']
            if not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', key) or not isinstance(value, str):
                raise Stop('Invalid Render environment entry')
            values[key] = value
        if len(page) < 100:
            return values
        cursor = page[-1].get('cursor')
        if not cursor or cursor in seen:
            raise Stop('Render pagination incomplete')
        seen.add(cursor)

def vercel_values(token):
    query = urlencode({'teamId':TEAM})
    data = get_json('https://api.vercel.com/v10/projects/' + PROJECT + '/env?' + query, token)
    rows = data if isinstance(data, list) else data.get('envs')
    if not isinstance(rows, list):
        raise Stop('Unexpected Vercel environment response')
    def retrieve(env_id):
        return get_json('https://api.vercel.com/v1/projects/' + PROJECT +
                        '/env/' + quote(str(env_id), safe='') + '?' + query, token)
    return production_values(rows, retrieve)

def private_write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.write_text(content)
    path.chmod(0o600)

def command(args, log, capture=False, env=None):
    with log.open('a') as output:
        r = subprocess.run(args, stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE if capture else output,
                           stderr=output, text=True, env=env)
    if r.returncode:
        raise Stop('Command failed: ' + args[0] + '; private log: ' + str(log))
    return r.stdout if capture else None

def source(repo, ref, path, log):
    if path.exists():
        head = command(['git','-C',str(path),'rev-parse','HEAD'], log, True).strip()
        dirty = command(['git','-C',str(path),'status','--porcelain'], log, True).strip()
        if head != ref or dirty:
            raise Stop('Existing source differs; left untouched: ' + str(path))
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, GIT_TERMINAL_PROMPT='0')
    clone = ['git','clone','--no-checkout','https://github.com/TechCodinz/' + repo + '.git',str(path)]
    try:
        command(clone, log, env=env)
    except Stop:
        if repo != 'OmniCode' or path.exists():
            raise
        print('OmniCode is private. Existing VPS GitHub authentication did not clone it.',flush=True)
        token = getpass.getpass('GitHub token with OmniCode Contents read access (hidden): ').strip()
        if not token:
            raise Stop('Private OmniCode source access is required; no services started')
        encoded = base64.b64encode(('x-access-token:' + token).encode()).decode()
        env.update(GIT_CONFIG_COUNT='1',
                   GIT_CONFIG_KEY_0='http.https://github.com/.extraheader',
                   GIT_CONFIG_VALUE_0='AUTHORIZATION: basic ' + encoded)
        command(clone, log, env=env)
        token = encoded = None
    command(['git','-C',str(path),'checkout','--detach',ref], log, env=env)

CKB_DOCKER = '''FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev cmake clang && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
ENV CARGO_BUILD_JOBS=2
RUN cargo build --locked --release -p ckb-mcp-server --bin render_gateway --bin reality_gateway --bin reality_server_v5 --bin ckb-mcp-server --bin model_registry_api && cargo build --locked --release -p ckb-cli --bin ckb-intelligence
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 libstdc++6 git curl && rm -rf /var/lib/apt/lists/* && useradd -m -u 10001 app
WORKDIR /app
COPY --from=build /build/target/release/render_gateway /build/target/release/reality_gateway /build/target/release/reality_server_v5 /build/target/release/ckb-mcp-server /build/target/release/model_registry_api /build/target/release/ckb-intelligence /app/target/release/
RUN mkdir -p /app/ckb_reality_data && chown -R app:app /app
USER app
ENV PORT=10000 CKB_REALITY_DATA_DIR=/app/ckb_reality_data
CMD ["./target/release/render_gateway"]
'''

OMNI_DOCKER = '''# syntax=docker/dockerfile:1
FROM node:22-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends openssl ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY package.json package-lock.json ./
COPY prisma ./prisma
COPY scripts ./scripts
RUN npm ci --no-audit --no-fund
COPY . .
ENV NEXT_TELEMETRY_DISABLED=1 NODE_OPTIONS=--max-old-space-size=3072
RUN --mount=type=secret,id=omni_env,target=/app/.env.production.local,required=true npm run build
RUN chown -R node:node /app
USER node
ENV NODE_ENV=production PORT=3000
CMD ["npm","run","start","--","--hostname","0.0.0.0"]
'''

def stage(name, config, dockerfile, source_dir, expected, backup):
    stack = BASE / 'stacks' / name
    stack.mkdir(parents=True, exist_ok=True)
    private_write(stack / 'Dockerfile', dockerfile)
    config_path = stack / 'compose.json'
    private_write(config_path, json.dumps(config, indent=2))
    # Keep secrets out of both build contexts, including locally generated files.
    private_write(source_dir / '.dockerignore', '.git\n.env\n.env.*\nnode_modules\n.next\ntarget\n')
    cmd = ['docker','compose','-p',name,'-f',str(config_path)]
    log = backup / (name + '.log')
    resolved = json.loads(command(cmd + ['config','--format','json'], log, True))
    for service, values in expected.items():
        actual = resolved['services'][service]['environment']
        if any(actual.get(k) != v for k,v in values.items()):
            raise Stop(name + ': environment round-trip mismatch')
    print(name + ': configuration verified; building...', flush=True)
    command(cmd + ['build'], log)
    command(cmd + ['up','-d'], log)
    command(cmd + ['ps'], log)
    print(name + ': start command completed; private log: ' + str(log), flush=True)

def main():
    os.umask(0o077)
    warnings.simplefilter('error', getpass.GetPassWarning)
    if os.geteuid() != 0 or not BASE.is_dir():
        raise Stop('Run as root on the existing app-platform VPS')
    for name in ('ckb-runtime','omnicode'):
        if (BASE / 'stacks' / name / 'compose.json').exists():
            raise Stop('Existing staged stack left untouched: ' + name)
    for port in (18083,18084,18085):
        with socket.socket() as sock:
            try: sock.bind(('127.0.0.1',port))
            except OSError: raise Stop('Port already occupied: ' + str(port)) from None
    if shutil.disk_usage(BASE).free < 15 * 1024**3:
        raise Stop('Less than 15 GiB disk space available')
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    backup = BASE / 'backups' / ('remaining-migration-' + stamp)
    backup.mkdir(parents=True, mode=0o700)
    log = backup / 'prepare.log'
    render_token = getpass.getpass('Render API key (hidden): ').strip()
    vercel_token = getpass.getpass('Vercel token (hidden): ').strip()
    if not render_token or not vercel_token:
        raise Stop('Both tokens are required; no services changed')
    mcp = render_values('srv-d9jpqugu01pc73c2vvjg', render_token)
    registry = render_values('srv-da3f0uht0dsc73fhm880', render_token)
    omni = vercel_values(vercel_token)
    render_token = vercel_token = None
    # Preserve the existing backend credential if Render omits fromService values.
    cloud_cmd = ['docker','compose','--env-file',str(BASE/'secrets/ckb-cloud.env'),
                 '-f',str(BASE/'stacks/ckb-cloud/compose.yml'),'config','--format','json']
    cloud = json.loads(command(cloud_cmd,log,True))['services']['api']['environment']
    if not mcp.get('CKB_INTERNAL_SECRET'):
        mcp['CKB_INTERNAL_SECRET'] = cloud.get('INTERNAL_API_SECRET','')
    for label, values, keys in (
        ('CKB MCP',mcp,['CKB_INTERNAL_SECRET']),
        ('CKB registry',registry,['CKB_API_KEY']),
        ('OmniCode',omni,['DATABASE_URL','JWT_SECRET','ENCRYPTION_KEY']),
    ):
        missing = [k for k in keys if not values.get(k)]
        if missing: raise Stop(label + ': missing existing settings: ' + ', '.join(missing))
    encryption = omni['ENCRYPTION_KEY']
    if len(encryption.encode()) != 32 and not re.fullmatch(r'[0-9a-fA-F]{64}',encryption):
        raise Stop('OmniCode existing encryption key has an unsupported format')
    if not any(omni.get(k) for k in ('GEMINI_API_KEY','OPENAI_API_KEY','GROQ_API_KEY','ANTHROPIC_API_KEY','XAI_API_KEY')):
        print('NOTICE: no AI provider credential exported; builder functionality will need an existing provider.')
    # Keep current public URLs and authentication audiences until deliberate cutover.
    mcp.update(PORT='10000',CKB_REALITY_DATA_DIR='/app/ckb_reality_data',
               CKB_REALITY_GATEWAY_BIN='/app/target/release/reality_gateway',
               CKB_REALITY_V5_BIN='/app/target/release/reality_server_v5',
               CKB_ALLOW_LOCAL_SCAN='0',CKB_MAX_CONCURRENT_SCANS='1')
    registry['PORT'] = '10000'
    omni.update(NODE_ENV='production',PORT='3000',OMNICODE_RUN_PRODUCTION_SCHEMA_GUARDS='0')
    omni.pop('VERCEL_OIDC_TOKEN',None)
    for label, values in [('ckb-mcp',mcp),('ckb-registry',registry),('omnicode',omni)]:
        private_write(backup/(label+'-environment.json'),json.dumps(values))
        print(label + ': recovered ' + str(len(values)) + ' settings (values hidden)',flush=True)
    ckb_source = BASE/'src/ckb-runtime'
    omni_source = BASE/'src/omnicode'
    source('CKB',CKB_REF,ckb_source,log)
    source('OmniCode',OMNI_REF,omni_source,log)
    # Only public build variables are needed by Next.js. Runtime secrets stay outside image layers.
    build_env = BASE/'secrets/omnicode-build.env'
    public_values = {k:v for k,v in omni.items() if k.startswith('NEXT_PUBLIC_')}
    if any('\n' in v or '\r' in v or '"' in v or '\\' in v for v in public_values.values()):
        raise Stop('Complex public build value requires review before dotenv export')
    private_write(build_env, ''.join(k+'="'+v.replace('$','\\$')+'"\n' for k,v in public_values.items()))
    common = {'restart':'unless-stopped','init':True,'cpus':1.5,'mem_limit':'2g'}
    ckb_config = {'services':{
        'mcp':{**common,'image':'techcodinz/ckb-runtime:vps-stage',
               'build':{'context':str(ckb_source),'dockerfile':str(BASE/'stacks/ckb-runtime/Dockerfile')},
               'ports':['127.0.0.1:18083:10000'],'environment':compose_env(mcp),
               'volumes':['ckb_reality_data:/app/ckb_reality_data']},
        'registry':{**common,'image':'techcodinz/ckb-runtime:vps-stage',
                    'command':['./target/release/model_registry_api'],
                    'ports':['127.0.0.1:18084:10000'],'environment':compose_env(registry)}},
        'volumes':{'ckb_reality_data':{}}}
    omni_config = {'services':{'web':{**common,'mem_limit':'3g',
        'image':'techcodinz/omnicode:vps-stage',
        'build':{'context':str(omni_source),'dockerfile':str(BASE/'stacks/omnicode/Dockerfile'),
                 'secrets':['omni_env']},
        'ports':['127.0.0.1:18085:3000'],'environment':compose_env(omni)}},
        'secrets':{'omni_env':{'file':str(build_env)}}}
    failures = []
    for args in [('ckb-runtime',ckb_config,CKB_DOCKER,ckb_source,{'mcp':mcp,'registry':registry}),
                 ('omnicode',omni_config,OMNI_DOCKER,omni_source,{'web':omni})]:
        try: stage(*args,backup)
        except Stop as e:
            failures.append(args[0]); print('STOPPED:',e,flush=True)
    for label,port,path in [('CKB MCP',18083,'/ready'),('CKB registry',18084,'/health'),('OmniCode',18085,'/api/ready')]:
        code = None
        for attempt in range(12):
            try:
                with urlopen('http://127.0.0.1:'+str(port)+path,timeout=10) as r: code=r.status
            except HTTPError as e: code=e.code
            except Exception: code=None
            if code == 200: break
            time.sleep(3)
        print(label + ': local readiness ' + str(code),flush=True)
    print('Staging finished. Build failures: ' + (', '.join(failures) or 'none'))
    print('Public routing, CKB persistent-data transfer, OAuth cutover and end-to-end tests remain pending.')
    print('Private backup/log directory:',backup)

if __name__ == '__main__':
    try: main()
    except Stop as e: print('STOPPED:',e)
    except Exception as e: print('STOPPED:',type(e).__name__,'(details withheld to protect secrets)')
