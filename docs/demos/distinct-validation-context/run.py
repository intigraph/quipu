import re,sqlite3,time
from pathlib import Path
source=(Path(__file__).resolve().parents[3] / 'src/store/shacl_poststate.rs').read_text()
query=re.search(r'"(SELECT e,a,v,MIN\(tx\),valid_from,valid_to FROM facts .*?LIMIT \?4)"',source,re.S).group(1)
query=re.sub(r'\\\n\s*',' ',query)
c=sqlite3.connect(':memory:')
c.execute('create table facts(e integer,a integer,v blob,g integer,tx integer,valid_from text,valid_to text,op integer,primary key(e,a,v,tx))')
c.executemany("insert into facts values(1,2,?,0,?,'2026-01-01',null,1)",((b'metadata',i) for i in range(100002)))
raw=c.execute('select count(*) from (select 1 from facts where e=1 and g=0 and op=1 and valid_to is null limit 100001)').fetchone()[0]
assert raw==100001
rows=c.execute(query,(1,0,None,100001)).fetchall();assert len(rows)==1 and rows[0][3]==0
assert c.execute(query,(1,0,3,100001)).fetchall()==[]
print('duplicate assertion fixture: old capped row count',raw,'candidate distinct triples',len(rows),'predicate-only absent context 0')
c.executemany("insert into facts values(1,3,?,0,0,'2026-01-01',null,1)",((str(i).encode(),) for i in range(100001)))
rows=c.execute(query,(1,0,None,100001)).fetchall();assert len(rows)==100001
assert len(c.execute(query,(1,0,2,100001)).fetchall())==1
print('distinct oversized fixture: candidate returns cap+1 sentinel 100001; predicate-filtered metadata context remains 1')
print('PASS: actual source SQL fixture; this demonstration does not execute the Rust write lane')
