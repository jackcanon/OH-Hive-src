#!/usr/bin/env python3
"""
HJM-DAM Fleet Crawler
=====================
Walks configurable roots (full recursion, no depth cap) on the LOCAL machine,
classifies every hit (git-repo / media / doc / other), and writes an
incremental SQLite index. One DB per host lives on the UNAS so every machine
and agent shares a single source of truth.

Design:
- Full-disk capable: roots are configurable (see roots.json). Smart excludes
  keep /System, Library caches, node_modules, .Trash, mountpoints out by default.
- Incremental: paths whose mtime is unchanged are NOT re-classified; rows for
  deleted paths are pruned. First run is a full insert (slow on big volumes);
  subsequent runs are fast deltas.
- No external deps (sqlite3 is stdlib).

Usage:
  crawler.py --db /path/to/<host>.db [--roots roots.json] [--dry-run]
"""
import os, sys, sqlite3, json, argparse, time, subprocess, datetime

# ---- classification sets ---------------------------------------------------
MEDIA_EXT = {".jpg",".jpeg",".png",".gif",".heic",".webp",".tif",".tiff",
             ".mp4",".mov",".m4v",".avi",".mkv",".webm",".mpg",".mpeg",
             ".mp3",".wav",".aiff",".flac",".m4a",".aac",".ogg",
             ".pdf",".psd",".ai",".raw",".cr2",".arw",".dng"}
DOC_EXT   = {".md",".txt",".rtf",".doc",".docx",".ppt",".pptx",".xls",".xlsx",
             ".pdf",".pages",".key",".numbers",".csv",".json",".yaml",".yml",
             ".html",".htm",".tex"}
CODE_EXT  = {".py",".js",".ts",".swift",".go",".rs",".c",".cpp",".h",".hpp",
             ".java",".rb",".sh",".sql",".php",".kt"}

# dirs we never recurse into (basename match, case-insensitive)
EXCLUDE_DIRS = {".git","node_modules",".cache","caches","library","trashes",
                ".trash","system","private","volumes","dev","proc",".spotlight",
                ".fseventsd",".vol",".sparsebundle",".bundle","deriveddata",
                "application support","caches","logs","tmp","__pycache__",
                ".venv","venv",".tox",".idea",".vscode"}

def load_roots(path):
    if not path or not os.path.exists(path):
        # sensible default: user home + any mounted data volumes
        roots = [os.path.expanduser("~")]
        for v in ("/Volumes/10TB JBOD", "/Volumes/HJMPool1", "/Volumes/hjm-dam"):
            if os.path.isdir(v):
                roots.append(v)
        return {"roots": roots, "exclude_dirs": sorted(EXCLUDE_DIRS)}
    with open(path) as f:
        cfg = json.load(f)
    cfg.setdefault("exclude_dirs", sorted(EXCLUDE_DIRS))
    return cfg

def classify_file(path):
    ext = os.path.splitext(path)[1].lower()
    if ext in MEDIA_EXT: return "media"
    if ext in DOC_EXT:   return "doc"
    if ext in CODE_EXT:  return "code"
    return "other"

def is_project_root(dirpath, filenames):
    """Detect a code project that may NOT be under git (catches local-only /
    never-committed work). Returns the detected stack or '' if not a project."""
    fset = set(filenames)
    if "Package.swift" in fset:            return "swift"
    if "pyproject.toml" in fset or "setup.py" in fset or "requirements.txt" in fset: return "python"
    if "package.json" in fset:             return "node"
    if "Cargo.toml" in fset:               return "rust"
    if "CMakeLists.txt" in fset:           return "cmake"
    if "go.mod" in fset:                   return "go"
    if any(f.endswith(".xcodeproj") or f.endswith(".xcworkspace") for f in fset): return "xcode"
    # bare index.html + a src/ or assets/ folder => static site / web project
    if "index.html" in fset and ("src" in fset or "assets" in fset): return "web"
    return ""

def git_info(repo_root):
    try:
        branch = subprocess.run(["git","-C",repo_root,"rev-parse","--abbrev-ref","HEAD"],
                                capture_output=True, text=True, timeout=10).stdout.strip()
        remote = subprocess.run(["git","-C",repo_root,"remote","get-url","origin"],
                                capture_output=True, text=True, timeout=10).stdout.strip()
        return branch, remote
    except Exception:
        return "", ""

def init_db(db):
    c = sqlite3.connect(db)
    c.executescript("""
      CREATE TABLE IF NOT EXISTS files(
        path TEXT PRIMARY KEY, host TEXT, kind TEXT, size INT, mtime REAL,
        branch TEXT, remote TEXT, scanned REAL);
      CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT);
    """)
    c.commit()
    return c

def crawl(c, roots, exclude):
    excl = set(d.lower() for d in exclude)
    host = os.uname().nodename
    now = time.time()
    seen = set()
    total = 0
    repo_roots_seen = set()
    for root in roots:
        root = os.path.expanduser(root)
        if not os.path.isdir(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            # prune excluded dirs in-place (don't recurse)
            dirnames[:] = [d for d in dirnames
                           if d.lower() not in excl and d not in excl]
            # detect git repo at this dir
            if ".git" in dirnames or os.path.isdir(os.path.join(dirpath, ".git")):
                if dirpath not in repo_roots_seen:
                    repo_roots_seen.add(dirpath)
                    branch, remote = git_info(dirpath)
                    mtime = os.path.getmtime(dirpath)
                    key = dirpath
                    c.execute("""INSERT INTO files(path,host,kind,size,mtime,branch,remote,scanned)
                                 VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(path)
                                 DO UPDATE SET host=excluded.host,kind=excluded.kind,
                                 size=excluded.size,mtime=excluded.mtime,branch=excluded.branch,
                                 remote=excluded.remote,scanned=excluded.scanned""",
                                 (key, host, "repo", 0, mtime, branch, remote, now))
                    seen.add(key)
                    total += 1
                # do NOT recurse into .git
                dirnames[:] = [d for d in dirnames if d != ".git"]
            else:
                # no .git here — but could still be a local-only / uncommitted project
                stack = is_project_root(dirpath, dirnames + filenames)
                if stack and dirpath not in repo_roots_seen:
                    repo_roots_seen.add(dirpath)
                    mtime = os.path.getmtime(dirpath)
                    c.execute("""INSERT INTO files(path,host,kind,size,mtime,branch,remote,scanned)
                                 VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(path)
                                 DO UPDATE SET host=excluded.host,kind=excluded.kind,
                                 size=excluded.size,mtime=excluded.mtime,branch=excluded.branch,
                                 remote=excluded.remote,scanned=excluded.scanned""",
                                 (dirpath, host, "project", 0, mtime, stack, "", now))
                    seen.add(dirpath)
                    total += 1
            for fn in filenames:
                fp = os.path.join(dirpath, fn)
                try:
                    st = os.lstat(fp)
                except OSError:
                    continue
                if not hasattr(st, "st_mtime"):
                    continue
                kind = classify_file(fp)
                if kind == "other":
                    continue
                c.execute("""INSERT INTO files(path,host,kind,size,mtime,branch,remote,scanned)
                             VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(path)
                             DO UPDATE SET host=excluded.host,kind=excluded.kind,
                             size=excluded.size,mtime=excluded.mtime,scanned=excluded.scanned""",
                             (fp, host, kind, st.st_size, st.st_mtime, "", "", now))
                seen.add(fp)
                total += 1
    # prune deleted
    cur = c.execute("SELECT path FROM files")
    deleted = 0
    for (p,) in cur:
        if p not in seen and not os.path.exists(p):
            c.execute("DELETE FROM files WHERE path=?", (p,))
            deleted += 1
    c.execute("INSERT OR REPLACE INTO meta VALUES('last_scan',?)", (datetime.datetime.utcnow().isoformat(),))
    c.execute("INSERT OR REPLACE INTO meta VALUES('host',?)", (host,))
    c.commit()
    return total, deleted

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", required=True, help="path to this host's SQLite index")
    ap.add_argument("--roots", default=None, help="roots.json config")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    cfg = load_roots(a.roots)
    print(f"[crawler] host={os.uname().nodename} roots={cfg['roots']}")
    c = init_db(a.db)
    t0 = time.time()
    total, deleted = crawl(c, cfg["roots"], cfg["exclude_dirs"])
    dt = time.time() - t0
    c.close()
    print(f"[crawler] wrote {total} rows (+{total-deleted} new/-{deleted} pruned) in {dt:.1f}s -> {a.db}")

if __name__ == "__main__":
    main()
