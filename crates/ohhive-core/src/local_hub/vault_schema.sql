CREATE TABLE vaults(id TEXT PRIMARY KEY, name TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'unavailable');
CREATE TABLE vault_documents(id TEXT PRIMARY KEY, vault_id TEXT NOT NULL REFERENCES vaults(id), path TEXT NOT NULL, revision TEXT NOT NULL, title TEXT NOT NULL, content TEXT NOT NULL, UNIQUE(vault_id,path));
CREATE TABLE vault_readers(vault_id TEXT NOT NULL REFERENCES vaults(id), node_id TEXT NOT NULL REFERENCES nodes(id), PRIMARY KEY(vault_id,node_id));
CREATE VIRTUAL TABLE vault_fts USING fts5(title,content,content='vault_documents',content_rowid='rowid');
CREATE TRIGGER vault_insert AFTER INSERT ON vault_documents BEGIN INSERT INTO vault_fts(rowid,title,content) VALUES(new.rowid,new.title,new.content); END;
CREATE TRIGGER vault_delete AFTER DELETE ON vault_documents BEGIN INSERT INTO vault_fts(vault_fts,rowid,title,content) VALUES('delete',old.rowid,old.title,old.content); END;
CREATE TRIGGER vault_update AFTER UPDATE ON vault_documents BEGIN
INSERT INTO vault_fts(vault_fts,rowid,title,content) VALUES('delete',old.rowid,old.title,old.content);
INSERT INTO vault_fts(rowid,title,content) VALUES(new.rowid,new.title,new.content);
END;
PRAGMA user_version=2;
