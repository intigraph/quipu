#!/usr/bin/env python3
"""Exercise the actual workflow shell with a private fake forge, no network."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

WORKFLOW = Path(os.environ.get('PENDANT_COMMENT_WORKFLOW',
    str(Path(__file__).resolve().parents[1]/'.github/workflows/pendant-review.yml')))
SOURCE = WORKFLOW.read_text()
SHELL = textwrap.dedent(SOURCE.split('      - name: Create or update the sticky comment\n', 1)[1]
                       .split('        run: |\n', 1)[1])

FAKE_GH = '''#!/usr/bin/env python3
import json,os,subprocess,sys
args=sys.argv[1:]
with open(os.environ['CALLS'],'a') as stream:stream.write(json.dumps(args)+'\\n')
if '-X' in args:
    print('{}')
elif '/pulls/' in ' '.join(args):
    if os.environ.get('API_FAIL')=='1':sys.exit(3)
    result=subprocess.run(['jq',args[args.index('--jq')+1]],input=os.environ['METADATA'],text=True,capture_output=True)
    print(result.stdout,end='');sys.exit(result.returncode)
else:
    print(os.environ.get('COMMENT_ID',''))
'''


class Guard(unittest.TestCase):
    def run_shell(self, metadata, *, failure=False, existing=False):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);binpath=root/'bin';binpath.mkdir()
            fake=binpath/'gh';fake.write_text(FAKE_GH);fake.chmod(0o700)
            (root/'pendant-review.md').write_text('fixture only')
            calls=root/'calls'
            env=dict(os.environ,PATH=str(binpath)+os.pathsep+os.environ['PATH'],
                     METADATA=json.dumps(metadata),CALLS=str(calls),REPO='intigraph/quipu',PR='1194',
                     API_FAIL='1' if failure else '0',COMMENT_ID='123' if existing else '',GH_TOKEN='fixture')
            result=subprocess.run(['bash','-c',SHELL],cwd=root,env=env,text=True,capture_output=True,timeout=10)
            rows=[json.loads(x) for x in calls.read_text().splitlines()]
            return result,[r for r in rows if '-X' in r]

    def metadata(self, login='scbrown', head='intigraph/quipu'):
        return {'user':{'login':login},'head':{'repo':{'full_name':head}},
                'base':{'repo':{'full_name':'intigraph/quipu'}}}

    def test_outside_same_repo_and_maintainer_edit_never_posts(self):
        value=self.metadata('outside');value['maintainer_can_modify']=True
        result,writes=self.run_shell(value)
        self.assertEqual(result.returncode,0,result.stderr);self.assertEqual(writes,[])

    def test_missing_author_and_fork_never_post(self):
        for value in [{},self.metadata(None),self.metadata(head='outside/quipu')]:
            with self.subTest(value=value):
                result,writes=self.run_shell(value)
                self.assertEqual(result.returncode,0,result.stderr);self.assertEqual(writes,[])

    def test_metadata_error_stops_before_write(self):
        result,writes=self.run_shell(self.metadata(),failure=True)
        self.assertNotEqual(result.returncode,0);self.assertEqual(writes,[])

    def test_maintainer_positive_posts_once(self):
        result,writes=self.run_shell(self.metadata())
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(len(writes),1);self.assertIn('POST',writes[0])

    def test_maintainer_positive_updates_once(self):
        result,writes=self.run_shell(self.metadata(),existing=True)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(len(writes),1);self.assertIn('PATCH',writes[0])

    def test_job_condition_excludes_outside_authors_before_write_permission(self):
        condition=SOURCE.split('  comment:\n',1)[1].split('    runs-on:',1)[0]
        self.assertIn("github.event.pull_request.user.login == 'scbrown'",condition)
        self.assertIn('github.event.pull_request.head.repo.full_name == github.repository',condition)


if __name__=='__main__':unittest.main()
