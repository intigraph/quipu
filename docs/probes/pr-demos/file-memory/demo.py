"""Isolated file-memory CLI demo. Usage: python memory-cli-demo.py BASE HEAD"""
import json, subprocess, sys, tempfile
from pathlib import Path
base,head=map(str,map(Path,sys.argv[1:3]))
owner={"deployment":"urn:example:deployment","project":"urn:example:project","crew":"urn:example:alpha"}
request={"owner":owner,"path":"MEMORY.md","text":"# Memory\r\nExact bytes: λ\r\n","expected_snapshot":None,"expected_file":None,"idempotency_key":"demo-1","harness":"fixture","account":"selected"}
with tempfile.TemporaryDirectory(prefix="memory-demo-") as directory:
 root=Path(directory); graph="urn:example:memory"
 req=root/"request.json"; req.write_text(json.dumps(request))
 def invoke(binary,args,db):
  return subprocess.run([binary,*args,"--db",str(root/db)],cwd=root,capture_output=True,text=True,timeout=30)
 old=invoke(base,["memory","commit",str(req),"--graph",graph,"--local-authority"],"base.db")
 print("BASE same commit request: exit="+str(old.returncode))
 assert old.returncode != 0
 assert invoke(head,["graph","create",graph],"head.db").returncode==0
 def commit():
  result=invoke(head,["memory","commit",str(req),"--graph",graph,"--local-authority"],"head.db")
  assert result.returncode==0
  return json.loads(result.stdout)
 first=commit(); replay=commit(); assert first==replay
 print("HEAD commit and replay: identical receipt="+str(first==replay))
 own=root/"owner.json";own.write_text(json.dumps(owner))
 result=invoke(head,["memory","export",str(own),"--graph",graph],"head.db")
 assert result.returncode==0
 export=json.loads(result.stdout)
 print("HEAD actual export: "+json.dumps(export,ensure_ascii=False,sort_keys=True))
 print("Target: synthetic local database; explicit filesystem authority. Native harness and remote identity NOT tested.")
