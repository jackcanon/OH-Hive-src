#!/usr/bin/env python3
"""Local staging index for Den. Originals stay in place; no network or inference.
Reuses HJM classification. Never prunes entries from offline/incomplete roots.
Not yet connected to the installed Library UI.
"""
import argparse, json, os, sqlite3, stat, time
from pathlib import Path
from hjm_crawler import classify_file, is_project_root, EXCLUDE_DIRS

TEXT = {'.md', '.markdown', '.txt', '.rst', '.py', '.js', '.ts', '.tsx', '.jsx',
        '.swift', '.rs', '.go', '.c', '.cpp', '.h', '.css', '.html', '.tex'}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--root', action='append', required=True)
    ap.add_argument('--output', required=True)
    a = ap.parse_args()
    os.umask(0o077)
    out = Path(a.output); out.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(out / 'library.sqlite3')
    db.executescript('''CREATE TABLE IF NOT EXISTS files(
      path TEXT PRIMARY KEY, host TEXT, root TEXT, kind TEXT, size INTEGER,
      mtime_ns INTEGER, seen REAL, text_status TEXT);
      CREATE VIRTUAL TABLE IF NOT EXISTS content USING fts5(path UNINDEXED, body);
    ''')
    totals = dict(visited=0, changed=0, text_indexed=0, errors=0)
    started = time.time()
    def report(state, root=''):
        db.commit()
        p = out / 'status.tmp'
        p.write_text(json.dumps(dict(state=state, root=root, started=started,
            updated=time.time(), **totals)))
        p.replace(out / 'status.json')
    excluded = {s.lower() for s in EXCLUDE_DIRS} | {'target','build','dist','vendor','pods'}
    host = os.uname().nodename
    def err(_): totals['errors'] += 1
    report('running')
    for root in a.root:
        root = os.path.abspath(os.path.expanduser(root))
        if not os.path.isdir(root):
            totals['errors'] += 1; continue
        for directory, dirs, files in os.walk(root, followlinks=False, onerror=err):
            project = 'repo' if os.path.exists(os.path.join(directory,'.git')) else (
                'project' if is_project_root(directory, dirs+files) else '')
            dirs[:] = [d for d in dirs if not d.startswith('.') and d.lower() not in excluded
                       and not d.lower().endswith(('.app','.bundle','.sparsebundle'))
                       and not os.path.islink(os.path.join(directory,d))
                       and os.path.realpath(os.path.join(directory,d)) != str(out.resolve())]
            entries = [(directory, project)] if project else []
            entries += [(os.path.join(directory,f), classify_file(f)) for f in files
                        if not f.startswith('.') and not f.lower().endswith(('.pem','.key','.p12','.pfx'))]
            for path, kind in entries:
                try:
                    st = os.lstat(path)
                    if stat.S_ISLNK(st.st_mode) or not (stat.S_ISREG(st.st_mode) or kind in ('repo','project')): continue
                    totals['visited'] += 1
                    old = db.execute('SELECT size,mtime_ns FROM files WHERE path=?',(path,)).fetchone()
                    if old == (st.st_size,st.st_mtime_ns):
                        db.execute('UPDATE files SET seen=? WHERE path=?',(time.time(),path))
                    else:
                        text_status = 'metadata_only'
                        db.execute('DELETE FROM content WHERE path=?',(path,))
                        if Path(path).suffix.lower() in TEXT and stat.S_ISREG(st.st_mode) and st.st_size <= 1024*1024:
                            try:
                                with open(path,'rb') as f: data=f.read(1024*1024+1)
                                if len(data)<=1024*1024 and b'\0' not in data:
                                    body=data.decode('utf-8')
                                    db.execute('INSERT INTO content(path,body) VALUES(?,?)',(path,body))
                                    text_status='indexed'; totals['text_indexed'] += 1
                            except (OSError,UnicodeError): text_status='unreadable'
                        db.execute('INSERT OR REPLACE INTO files VALUES(?,?,?,?,?,?,?,?)',
                            (path,host,root,kind,st.st_size,st.st_mtime_ns,time.time(),text_status))
                        totals['changed'] += 1
                    if totals['visited'] % 500 == 0: report('running',root)
                except OSError: totals['errors'] += 1
            if totals['visited'] % 5000 < 100: time.sleep(.01)
        report('running',root)
    report('completed_with_skips' if totals['errors'] else 'completed')
    db.close()
if __name__ == '__main__': main()
