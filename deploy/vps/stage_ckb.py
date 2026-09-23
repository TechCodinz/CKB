#!/usr/bin/env python3
"""Stage only CKB's runtime; no Vercel credentials or public cutover."""
import json
import os
import shutil
import socket
import warnings
from datetime import datetime, timezone
import stage as shared


def runtime_config(source_dir, mcp, registry):
    common = {'restart':'unless-stopped','init':True,'cpus':1.5,'mem_limit':'2g'}
    return {'services':{
        'mcp':{**common,'image':'techcodinz/ckb-runtime:vps-stage',
               'build':{'context':str(source_dir),
                        'dockerfile':str(shared.BASE/'stacks/ckb-runtime/Dockerfile')},
               'ports':['127.0.0.1:18083:10000'],'environment':shared.compose_env(mcp),
               'volumes':['ckb_reality_data:/app/ckb_reality_data']},
        'registry':{**common,'image':'techcodinz/ckb-runtime:vps-stage',
                    'command':['./target/release/model_registry_api'],
                    'ports':['127.0.0.1:18084:10000'],
                    'environment':shared.compose_env(registry)}},
        'volumes':{'ckb_reality_data':{}}}


def main():
    os.umask(0o077)
    warnings.simplefilter('error', shared.getpass.GetPassWarning)
    base = shared.BASE
    if os.geteuid() != 0 or not base.is_dir():
        raise shared.Stop('Run as root on the existing app-platform VPS')
    if (base/'stacks/ckb-runtime/compose.json').exists():
        raise shared.Stop('Existing CKB runtime stack left untouched; inspect it before resuming')
    for port in (18083,18084):
        with socket.socket() as sock:
            try: sock.bind(('127.0.0.1',port))
            except OSError: raise shared.Stop('Port already occupied: ' + str(port)) from None
    if shutil.disk_usage(base).free < 15 * 1024**3:
        raise shared.Stop('Less than 15 GiB disk space available')
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    backup = base/'backups'/('ckb-runtime-stage-' + stamp)
    backup.mkdir(parents=True, mode=0o700)
    log = backup/'prepare.log'
    token = shared.getpass.getpass('Render API key (hidden): ').strip()
    if not token: raise shared.Stop('Render API key required; no services changed')
    mcp = shared.render_values('srv-d9jpqugu01pc73c2vvjg',token)
    registry = shared.render_values('srv-da3f0uht0dsc73fhm880',token)
    token = None
    cloud_cmd = ['docker','compose','--env-file',str(base/'secrets/ckb-cloud.env'),
                 '-f',str(base/'stacks/ckb-cloud/compose.yml'),'config','--format','json']
    cloud = json.loads(shared.command(cloud_cmd,log,True))['services']['api']['environment']
    if not mcp.get('CKB_INTERNAL_SECRET'):
        mcp['CKB_INTERNAL_SECRET'] = cloud.get('INTERNAL_API_SECRET','')
    for label,values,key in [('CKB MCP',mcp,'CKB_INTERNAL_SECRET'),
                             ('CKB registry',registry,'CKB_API_KEY')]:
        if not values.get(key): raise shared.Stop(label + ': missing existing setting ' + key)
    mcp.update(PORT='10000',CKB_REALITY_DATA_DIR='/app/ckb_reality_data',
               CKB_REALITY_GATEWAY_BIN='/app/target/release/reality_gateway',
               CKB_REALITY_V5_BIN='/app/target/release/reality_server_v5',
               CKB_ALLOW_LOCAL_SCAN='0',CKB_MAX_CONCURRENT_SCANS='1')
    registry['PORT'] = '10000'
    for label,values in [('ckb-mcp',mcp),('ckb-registry',registry)]:
        shared.private_write(backup/(label+'-environment.json'),json.dumps(values))
        print(label + ': existing settings recovered (values hidden)',flush=True)
    source_dir = base/'src/ckb-runtime'
    shared.source('CKB',shared.CKB_REF,source_dir,log)
    shared.stage('ckb-runtime',runtime_config(source_dir,mcp,registry),shared.CKB_DOCKER,
                 source_dir,{'mcp':mcp,'registry':registry},backup)
    failed = []
    for label,port,path in [('CKB MCP',18083,'/ready'),('CKB registry',18084,'/health')]:
        code = None
        for attempt in range(12):
            try:
                with shared.urlopen('http://127.0.0.1:'+str(port)+path,timeout=10) as r:
                    code = r.status
            except shared.HTTPError as e: code = e.code
            except Exception: code = None
            if code == 200: break
            shared.time.sleep(3)
        print(label + ': local readiness ' + str(code),flush=True)
        if code != 200: failed.append(label)
    print('Private logs:',backup)
    if failed: raise shared.Stop('Readiness failed: ' + ', '.join(failed))
    print('CKB runtime staged. Public routing, persistent-data transfer and functional checks remain pending.')


if __name__ == '__main__':
    try: main()
    except shared.Stop as error:
        print('STOPPED:',error)
        raise SystemExit(1)
    except Exception as error:
        print('STOPPED:',type(error).__name__,'(details withheld to protect secrets)')
        raise SystemExit(1)
