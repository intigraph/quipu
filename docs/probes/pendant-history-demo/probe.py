import json, os, pathlib, subprocess, tempfile, sys
script, binary = map(str,sys.argv[1:])
with tempfile.TemporaryDirectory() as d:
 p=pathlib.Path(d)
 def git(*args):
  return subprocess.check_output(['git','-C',d,*args],stderr=subprocess.DEVNULL,text=True).strip()
 git('init','-b','main');git('config','user.name','Probe');git('config','user.email','probe@example.org')
 (p/'README').write_text('base');git('add','.');git('commit','-m','base');base=git('rev-parse','HEAD')
 git('checkout','-b','left');(p/'fixture').mkdir();(p/'fixture/export.nt').write_text('<urn:a> <urn:b> <urn:c> .\n');git('add','.');git('commit','-m','nonpack fixture')
 git('checkout','-b','right',base);(p/'other').write_text('right');git('add','.');git('commit','-m','other')
 git('checkout','left');git('merge','--no-ff','right','-m','historical merge');historical=git('rev-parse','HEAD')
 git('mv','fixture/export.nt','fixture/payload.nq');git('commit','-am','rename fixture')
 event=p/'event.json';event.write_text(json.dumps({'before':base}))
 result=subprocess.run(['python3',script,'--binary',binary],cwd=d,env={**os.environ,'GITHUB_EVENT_NAME':'push','GITHUB_EVENT_PATH':str(event)},text=True,capture_output=True)
 print(json.dumps({'historical_nonpack_repaired_head_pass':result.returncode==0,'returncode':result.returncode,'stdout':result.stdout,'stderr':result.stderr.splitlines()[0] if result.stderr else ''}))
 sys.exit(result.returncode)
