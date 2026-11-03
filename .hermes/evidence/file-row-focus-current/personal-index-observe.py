from pathlib import Path
import subprocess,hashlib,json,os,time
r=Path.cwd();e=Path(__file__).resolve().parent
pid=int(subprocess.check_output(['systemctl','--user','show','ottrin-index.service','--property=MainPID','--value'],text=True).strip());assert pid>1
p=Path('/proc')/str(pid);config=Path.home()/'.config/ottrin/config.json'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def sample():
 stat=(p/'stat').read_text();fields=stat[stat.rfind(')')+2:].split();return {'at':time.monotonic(),'start_ticks':fields[19],'cpu_ticks':int(fields[11])+int(fields[12]),'rss_bytes':int((p/'statm').read_text().split()[1])*os.sysconf('SC_PAGE_SIZE')}
comm=(p/'comm').read_text().strip();binary=sha(p/'exe');assert comm=='ottrin-indexd' and binary=='21fe794ad24e6db1c38c30a0fa34ef34b258d7d8dd09f78fa8ec21667dbcbdc0'
before_config=sha(config);a=sample();time.sleep(30);b=sample();assert a['start_ticks']==b['start_ticks'] and sha(p/'exe')==binary and sha(config)==before_config
elapsed=b['at']-a['at'];out={'status':'bounded live personal daemon observation; sustained workload acceptance separate','pid':pid,'process_name':comm,'executable_sha256':binary,'seconds':elapsed,'cpu_ticks_delta':b['cpu_ticks']-a['cpu_ticks'],'percent_one_core':100*(b['cpu_ticks']-a['cpu_ticks'])/os.sysconf('SC_CLK_TCK')/elapsed,'rss_before_bytes':a['rss_bytes'],'rss_after_bytes':b['rss_bytes'],'config_sha256_unchanged':before_config,'interventions':[],'scope':'Read existing unit PID/proc counters/executable and config hash only. No daemon launch/restart, index query, database operation or config mutation. Sample occurs after owned builds/native fixtures finish; no guarantee of whole-machine inactivity or sustained workloads.'}
(e/'PERSONAL_INDEX_OBSERVATION.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out))
