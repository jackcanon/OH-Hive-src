from pathlib import Path
import os,subprocess,time,urllib.request,urllib.error,json
T=Path('/opt/hive-control-pilot/reconnect-test');B=T/'runtime/usr/lib/postgresql/16/bin'
env=os.environ.copy();env.update(LD_LIBRARY_PATH=str(T/'runtime/usr/lib/x86_64-linux-gnu'))
def pg(action):
 subprocess.run(['runuser','-u','nobody','--','env','LD_LIBRARY_PATH='+env['LD_LIBRARY_PATH'],str(B/'pg_ctl'),'-D',str(T/'data'),'-l',str(T/'data/postgres.log'),action],check=True,stdout=subprocess.DEVNULL)
def status():
 try:
  with urllib.request.urlopen('http://127.0.0.1:8792/hive/ctl/1/ready',timeout=4) as r:return r.status
 except urllib.error.HTTPError as e:return e.code
 except (urllib.error.URLError,TimeoutError):return 0
def wait(want,limit=40):
 start=time.monotonic()
 while time.monotonic()-start<limit:
  if status()==want:return round(time.monotonic()-start,3)
  time.sleep(.2)
 raise RuntimeError(f'expected readiness {want}, got {status()}')
pg('stop')
env.update(HIVE_CTL_DATABASE_URL='host=127.0.0.1 port=55439 user=sif_test dbname=postgres',HIVE_CTL_DATABASE_CA=str(T/'data/server.crt'),HIVE_CTL_LISTEN='127.0.0.1:8792')
log=(T/'pilot-process.log').open('w')
p=subprocess.Popen(['/opt/hive-control-pilot/source/target/debug/hive-control-pilot'],env=env,stdout=log,stderr=log)
results={}
try:
 results['startup_without_database_503_seconds']=wait(503)
 pg('start');results['automatic_first_connection_seconds']=wait(200)
 pid=p.pid
 pg('stop');results['database_loss_503_seconds']=wait(503)
 pg('start');results['automatic_reconnect_seconds']=wait(200)
 assert p.poll() is None and p.pid==pid
 results['same_pilot_process_after_reconnect']=True
 # Simulate an invalid/disabled readiness contract without disconnecting PostgreSQL.
 subprocess.run([str(B/'psql'),'-h','127.0.0.1','-p','55439','-U','sif_test','-d','postgres','-c','drop function hive.ctl_pilot_ready();'],env=env,check=True,stdout=subprocess.DEVNULL)
 results['missing_gateway_503_seconds']=wait(503)
 subprocess.run([str(B/'psql'),'-h','127.0.0.1','-p','55439','-U','sif_test','-d','postgres','-c','create function hive.ctl_pilot_ready() returns boolean language sql as $$ select true $$;'],env=env,check=True,stdout=subprocess.DEVNULL)
 results['restored_gateway_ready_seconds']=wait(200)
 print(json.dumps(results,indent=2),flush=True)
finally:
 p.terminate()
 try:p.wait(timeout=15)
 except subprocess.TimeoutExpired:p.kill();p.wait()
 pg('stop');log.close()
 print('Isolated pilot and disposable PostgreSQL stopped; production service untouched',flush=True)
